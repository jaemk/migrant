/*!
Configuration
*/
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use log::{debug, error};

use crate::drivers::DbConnection;
use crate::errors::*;
use crate::macros::{bail, err};
use crate::migratable::Migratable;
use crate::{tags, DbKind, DT_FORMAT, SQLITE_MEMORY_PATH};

mod builders;
mod init;
mod settings;

pub use builders::{MySqlSettingsBuilder, PostgresSettingsBuilder, SqliteSettingsBuilder};
pub use init::SettingsFileInitializer;
pub use settings::Settings;

pub(crate) use settings::DbSettings;

/// Full project configuration
///
/// Holds connection settings, the set of migrations to manage, and a lazily
/// established database connection. The connection is opened on first use and
/// kept alive for the life of this `Config` and all of its clones -- this is
/// what makes in-memory sqlite databases (`:memory:`) usable: every operation
/// sees the same live database.
#[derive(Debug, Clone)]
pub struct Config {
    pub(crate) settings: Settings,
    pub(crate) settings_path: Option<PathBuf>,
    pub(crate) applied: Vec<String>,
    pub(crate) migrations: Option<Vec<Box<dyn Migratable>>>,
    pub(crate) cli_compatible: bool,
    conn: Arc<Mutex<Option<DbConnection>>>,
    /// Bumped whenever an established connection is dropped (and will be
    /// re-established on next use). Session-scoped state -- notably the
    /// migration advisory lock -- does not survive such a drop, so the
    /// migrator uses this to detect a lost lock mid-run. Shared with the
    /// connection itself: clones and reloads that carry the connection over
    /// carry the generation with it.
    conn_generation: Arc<AtomicU64>,
}

impl Config {
    fn from_parts(settings: Settings, settings_path: Option<PathBuf>) -> Self {
        Self {
            settings,
            settings_path,
            applied: vec![],
            migrations: None,
            cli_compatible: false,
            conn: Arc::new(Mutex::new(None)),
            conn_generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Initialize a `Config` from a settings file at the given path.
    /// This does not query the database for applied migrations.
    pub fn from_settings_file<T: AsRef<Path>>(path: T) -> Result<Config> {
        let path = path.as_ref();
        let settings = Settings::from_file(path)?;
        Ok(Self::from_parts(settings, Some(path.to_owned())))
    }

    /// Initialize a `Config` using an explicitly created `Settings` object.
    /// This alleviates the need for a settings file.
    /// This does not query the database for applied migrations.
    ///
    /// ```rust,no_run
    /// # use migrant_lib::{Settings, Config};
    /// # fn main() { run().unwrap(); }
    /// # fn run() -> Result<(), Box<dyn std::error::Error>> {
    /// let settings = Settings::configure_sqlite()
    ///     .database_path("/absolute/path/to/db.db")?
    ///     .migration_location("/absolute/path/to/migration_dir")?
    ///     .build()?;
    /// let config = Config::with_settings(settings);
    /// // Setup migrations table
    /// config.setup()?;
    ///
    /// // Reload config, ping the database for applied migrations
    /// let config = config.reload()?;
    /// # let _ = config;
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_settings(settings: Settings) -> Config {
        Self::from_parts(settings, None)
    }

    /// Initialize a new settings file in the given directory
    pub fn init_in<T: AsRef<Path>>(dir: T) -> SettingsFileInitializer {
        SettingsFileInitializer::new(dir.as_ref())
    }

    /// Run a closure with the live database connection, establishing
    /// the connection first if necessary.
    pub(crate) fn with_conn<T>(&self, f: impl FnOnce(&mut DbConnection) -> Result<T>) -> Result<T> {
        let mut guard = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        if guard.is_none() {
            *guard = Some(DbConnection::connect(self)?);
        }
        let conn = guard.as_mut().expect("connection just established");
        let res = f(conn);
        if res.is_err() && self.database_type() != DbKind::Sqlite {
            // A server error may leave the connection stuck in an
            // aborted-transaction state. Recover it in place with a rollback
            // rather than dropping it, so session-scoped state -- notably the
            // migration advisory lock -- survives the error and is still held
            // when a `force`d run continues past it. Only if the rollback fails
            // (a genuinely dead connection) do we drop it, so the next
            // operation reconnects. Sqlite is excluded: its driver handles
            // rollback and an in-memory database would be lost if dropped.
            if conn.rollback().is_err() {
                *guard = None;
                self.conn_generation.fetch_add(1, Ordering::SeqCst);
            }
        }
        res
    }

    /// Generation of the current connection. Changes whenever an established
    /// connection had to be dropped (see `conn_generation`).
    pub(crate) fn connection_generation(&self) -> u64 {
        self.conn_generation.load(Ordering::SeqCst)
    }

    /// Execute a batch of sql statements on the database
    pub(crate) fn execute_sql(&self, sql: &str) -> Result<()> {
        self.with_conn(|conn| conn.execute_batch(sql))
    }

    /// Begin a transaction on the live connection
    pub(crate) fn begin_transaction(&self) -> Result<()> {
        self.with_conn(|conn| conn.begin())
    }

    /// Commit the current transaction on the live connection
    pub(crate) fn commit_transaction(&self) -> Result<()> {
        self.with_conn(|conn| conn.commit())
    }

    /// Roll back the current transaction on the live connection.
    ///
    /// Best-effort: this runs on error paths where the connection may already
    /// have been torn down (which itself rolls back the transaction on server
    /// databases), so failures to issue an explicit `rollback` are ignored.
    pub(crate) fn rollback_transaction(&self) {
        let _ = self.with_conn(|conn| conn.rollback());
    }

    /// Acquire the session-level advisory lock that serializes migration runs.
    /// Blocks until the lock is available. No-op for sqlite.
    pub(crate) fn acquire_migration_lock(&self) -> Result<()> {
        self.with_conn(|conn| conn.acquire_lock())
    }

    /// Release the migration advisory lock.
    ///
    /// Best-effort: server databases release session-level locks automatically
    /// when the connection drops, so failures to issue an explicit unlock (for
    /// example after the connection was reconnected on an error path) are
    /// ignored.
    pub(crate) fn release_migration_lock(&self) {
        let _ = self.with_conn(|conn| conn.release_lock());
    }

    /// Return a shared handle to the live sqlite connection,
    /// establishing the connection first if necessary.
    ///
    /// This is the same connection used to apply migrations. It is kept
    /// alive by this `Config` and all of its clones, so for in-memory
    /// (`:memory:`) databases this is the way to query the migrated
    /// database from application code:
    ///
    /// ```rust,no_run
    /// # fn run() -> Result<(), Box<dyn std::error::Error>> {
    /// let settings = migrant_lib::Settings::configure_sqlite().memory().build()?;
    /// let config = migrant_lib::Config::with_settings(settings);
    /// config.setup()?;
    /// // ... apply migrations ...
    /// let conn = config.sqlite_connection()?;
    /// let conn = conn.lock().unwrap();
    /// let n: i64 = conn.query_row("select count(*) from users", [], |r| r.get(0))?;
    /// # let _ = n;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "sqlite")]
    pub fn sqlite_connection(&self) -> Result<Arc<Mutex<rusqlite::Connection>>> {
        self.with_conn(|conn| match conn {
            DbConnection::Sqlite(s) => Ok(s.handle()),
            #[allow(unreachable_patterns)]
            _ => Err(err!(
                Config,
                "Cannot get a sqlite connection for database-type: {}",
                self.database_type()
            )),
        })
    }

    /// Define an explicit set of `Migratable` migrations to use.
    ///
    /// The order of definition is the order in which they will be applied.
    ///
    /// **Note:** When using explicit migrations, make sure any toggling of `Config::use_cli_compatible_tags`
    /// happens **before** the call to `Config::use_migrations`.
    ///
    /// # Example
    ///
    /// The following uses a migrant config file for connection configuration and
    /// explicitly defines migrations with `use_migrations`.
    ///
    /// ```rust,no_run
    /// use migrant_lib::{
    ///     Config, search_for_settings_file,
    ///     EmbeddedMigration, FileMigration, FnMigration
    /// };
    ///
    /// # fn run() -> Result<(), Box<dyn std::error::Error>> {
    /// mod migrations {
    ///     pub struct Custom;
    ///     impl Custom {
    ///         pub fn up(_: migrant_lib::ConnConfig) -> Result<(), Box<dyn std::error::Error>> {
    ///             print!(" <[Up!]>");
    ///             Ok(())
    ///         }
    ///         pub fn down(_: migrant_lib::ConnConfig) -> Result<(), Box<dyn std::error::Error>> {
    ///             print!(" <[Down!]>");
    ///             Ok(())
    ///         }
    ///     }
    /// }
    ///
    /// let p = search_for_settings_file(&std::env::current_dir()?)
    ///     .ok_or_else(|| "Settings file not found")?;
    /// let mut config = Config::from_settings_file(&p)?;
    /// # #[cfg(any(feature="sqlite", feature="postgres", feature="mysql"))]
    /// config.use_migrations(&[
    ///     EmbeddedMigration::with_tag("create-users-table")
    ///         .up(include_str!("../../migrations/embedded/create_users_table/up.sql"))
    ///         .down(include_str!("../../migrations/embedded/create_users_table/down.sql"))
    ///         .boxed(),
    ///     FileMigration::with_tag("create-places-table")
    ///         .up("migrations/embedded/create_places_table/up.sql")
    ///         .down("migrations/embedded/create_places_table/down.sql")
    ///         .boxed(),
    ///     FnMigration::with_tag("custom")
    ///         .up(migrations::Custom::up)
    ///         .down(migrations::Custom::down)
    ///         .boxed(),
    /// ])?;
    ///
    /// // Load applied migrations
    /// let config = config.reload()?;
    /// # let _ = config;
    /// # Ok(())
    /// # }
    /// # fn main() { run().unwrap(); }
    /// ```
    pub fn use_migrations<T: AsRef<[Box<dyn Migratable>]>>(
        &mut self,
        migrations: T,
    ) -> Result<&mut Self> {
        let migrations = migrations.as_ref();
        let mut seen = HashSet::with_capacity(migrations.len());
        for mig in migrations {
            let tag = mig.tag();
            if self.cli_compatible {
                if !tags::is_valid_full_tag(&tag) {
                    bail!(
                        TagError,
                        "When `cli_compatible=true` tags must be timestamped, \
                         following: `[0-9]{{14}}_[a-z0-9-]+`. Found tag: `{}`",
                        tag
                    )
                }
            } else if !tags::is_valid_opt_stamped_tag(&tag) {
                bail!(
                    TagError,
                    "When `cli_compatible=false` (default) tags may only contain, \
                     `[a-z0-9-]` and may be optionally prefixed with a timestamp \
                     following: `([0-9]{{14}}_)?[a-z0-9-]+`. Found tag: `{}`",
                    tag
                )
            }
            if !seen.insert(tag.clone()) {
                bail!(TagError, "Tags must be unique. Found duplicate: {}", tag)
            }
        }
        self.migrations = Some(migrations.to_vec());
        Ok(self)
    }

    /// Migrations are explicitly defined
    pub fn is_explicit(&self) -> bool {
        self.migrations.is_some()
    }

    /// Toggle cli compatible tag validation.
    ///
    /// Returns `&mut Self` so it can be chained directly onto construction,
    /// before `use_migrations`/`reload`:
    ///
    /// ```rust,no_run
    /// # fn run() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut config = migrant_lib::Config::from_settings_file("Migrant.toml")?;
    /// config.use_cli_compatible_tags(true);
    /// let config = config.reload()?;
    /// # let _ = config;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// **Note:** Because both `Config::use_migrations` and `Config::reload`
    /// depend on the tag format in use, set this **before** calling either.
    ///
    /// Defaults to `false`. When `cli_compatible` is set to `true`, migration
    /// tags will be validated in a manner compatible with the migrant CLI tool.
    /// Tags must be prefixed with a timestamp, following: `[0-9]{14}_[a-z0-9-]+`.
    /// When not enabled (the default), tag timestamps are optional and
    /// the migrant CLI tool will not be able to identify tags.
    pub fn use_cli_compatible_tags(&mut self, compat: bool) -> &mut Self {
        self.cli_compatible = compat;
        self
    }

    /// Check the current cli compatibility
    pub fn is_cli_compatible(&self) -> bool {
        self.cli_compatible
    }

    /// Check that migration tags conform to naming requirements.
    /// If CLI compatibility is enabled, then tags must be prefixed with a timestamp
    /// following: `[0-9]{14}_[a-z0-9-]+` which is the format generated by the migrant
    /// CLI tool and `migrant_lib::new`. When CLI compatibility is disabled (default),
    /// tags may only contain `[a-z0-9-]`, but can still be optionally prefixed with
    /// a timestamp following: `([0-9]{14}_)?[a-z0-9-]+`.
    fn check_saved_tag(&self, tag: &str) -> Result<()> {
        if self.cli_compatible {
            if !tags::is_valid_full_tag(tag) {
                bail!(
                    Migration,
                    "Found a non-conforming tag in the database: `{}`. \
                     Generated/CLI-compatible tags must follow `[0-9]{{14}}_[a-z0-9-]+`",
                    tag
                )
            }
        } else if !tags::is_valid_opt_stamped_tag(tag) {
            bail!(
                Migration,
                "Found a non-conforming tag in the database: `{}`. \
                 Managed/embedded tags may contain `[a-z0-9-]+`",
                tag
            )
        }
        Ok(())
    }

    /// Queries the database to reload the current applied migrations.
    ///
    /// **Note:** Make sure any calls to `Config::use_cli_compatible_tags` happen
    /// **before** any calls to `Config::reload` since this is dependent on the
    /// tag format being used.
    ///
    /// If the `Config` was initialized from a settings file, the settings
    /// will also be reloaded from the file. Returns a new `Config` instance.
    pub fn reload(&self) -> Result<Config> {
        let mut config = match self.settings_path.as_ref() {
            Some(path) => {
                let mut reloaded = Config::from_settings_file(path)?;
                // `from_settings_file` builds a fresh, unconnected config. If the
                // settings on disk are unchanged, carry over our live connection
                // so that in-memory (`:memory:`) databases -- whose entire state
                // lives in the connection handle -- are not silently discarded.
                // If the settings changed, keep the fresh connection so the
                // reloaded config connects using the new settings.
                if reloaded.settings == self.settings {
                    reloaded.conn = Arc::clone(&self.conn);
                    reloaded.conn_generation = Arc::clone(&self.conn_generation);
                }
                reloaded
            }
            None => self.clone(),
        };
        config.cli_compatible = self.cli_compatible;
        config.migrations = self.migrations.clone();
        config.applied = config.load_applied()?;
        Ok(config)
    }

    /// Re-read applied migrations from the database in place, without
    /// re-reading the settings file or touching the live connection (unlike
    /// `Config::reload`). Used by the migrator so a run stays on the
    /// connection its advisory lock was acquired on.
    pub(crate) fn refresh_applied(&mut self) -> Result<()> {
        self.applied = self.load_applied()?;
        Ok(())
    }

    /// Load the applied migrations from the database migration table
    pub(crate) fn load_applied(&self) -> Result<Vec<String>> {
        if !self.migration_table_exists()? {
            bail!(
                Migration,
                "`__migrant_migrations` table is missing, maybe try re-setting-up? -> `setup`"
            )
        }

        let applied = self.with_conn(|conn| conn.applied_tags())?;
        for tag in &applied {
            self.check_saved_tag(tag)?;
        }
        if !self.cli_compatible {
            return Ok(applied);
        }
        // Applied cli-compatible (timestamp-prefixed) tags are ordered chronologically
        let mut stamped = applied
            .into_iter()
            .map(|tag| {
                let stamp = tag
                    .split('_')
                    .next()
                    .ok_or_else(|| err!(TagError, "Invalid tag format: {:?}", tag))?;
                let stamp = chrono::NaiveDateTime::parse_from_str(stamp, DT_FORMAT)?;
                Ok((stamp, tag))
            })
            .collect::<Result<Vec<_>>>()?;
        stamped.sort_by_key(|(stamp, _)| *stamp);
        Ok(stamped.into_iter().map(|(_, tag)| tag).collect())
    }

    /// Check if a `__migrant_migrations` table exists
    pub(crate) fn migration_table_exists(&self) -> Result<bool> {
        self.with_conn(|conn| conn.migration_table_exists())
    }

    /// Insert given tag into database migration table
    pub(crate) fn insert_migration_tag(&self, tag: &str) -> Result<()> {
        self.with_conn(|conn| conn.insert_tag(tag))
    }

    /// Remove a given tag from the database migration table
    pub(crate) fn delete_migration_tag(&self, tag: &str) -> Result<()> {
        self.with_conn(|conn| conn.remove_tag(tag))
    }

    /// Confirm the database can be accessed and setup the database
    /// migrations table if it doesn't already exist
    pub fn setup(&self) -> Result<bool> {
        debug!(" ** Confirming database credentials...");
        match self.settings.inner {
            DbSettings::Sqlite(ref s) => {
                if !s.is_memory() {
                    let path = self.database_path()?;
                    debug!("    - checking if db file already exists...");
                    if create_file_if_missing(&path)? {
                        debug!("    - db not found... creating now... ✓");
                    } else {
                        debug!("    - db already exists ✓");
                    }
                }
            }
            DbSettings::Postgres(ref s) => {
                if let Err(e) = self.with_conn(|_| Ok(())) {
                    error!(" ERROR: Unable to connect to postgres database");
                    error!("        Please initialize your database and user and then run `setup`");
                    error!("\n  ex) sudo -u postgres createdb {}", s.database_name);
                    error!("      sudo -u postgres createuser {}", s.database_user);
                    error!(
                        "      sudo -u postgres psql -c \"alter user {} with password '****'\"",
                        s.database_user
                    );
                    error!("");
                    bail!(
                        Config,
                        "Cannot connect to postgres database. \
                         Do the database & user exist? -> {}",
                        e
                    );
                }
                debug!("    - Connection confirmed ✓");
            }
            DbSettings::MySql(ref s) => {
                if let Err(e) = self.with_conn(|_| Ok(())) {
                    let localhost = String::from("localhost");
                    let host = s.database_host.as_ref().unwrap_or(&localhost);
                    error!(" ERROR: Unable to connect to mysql database");
                    error!("        Please initialize your database and user and then run `setup`");
                    error!(
                        "\n  ex) mysql -u root -p -e \"create database {};\"",
                        s.database_name
                    );
                    error!(
                        "      mysql -u root -p -e \"create user '{}'@'{}' identified by '*****';\"",
                        s.database_user, host
                    );
                    error!(
                        "      mysql -u root -p -e \"grant all privileges on {}.* to '{}'@'{}';\"",
                        s.database_name, s.database_user, host
                    );
                    error!("      mysql -u root -p -e \"flush privileges;\"");
                    error!("");
                    bail!(
                        Config,
                        "Cannot connect to mysql database. \
                         Do the database & user exist? -> {}",
                        e
                    );
                }
                debug!("    - Connection confirmed ✓");
            }
        }

        debug!("\n ** Setting up migrations table");
        let table_created = self.with_conn(|conn| conn.setup_migration_table())?;
        if table_created {
            debug!("    - migrations table missing");
            debug!("    - `__migrant_migrations` table created ✓");
        } else {
            debug!("    - `__migrant_migrations` table already exists ✓");
        }
        Ok(table_created)
    }

    /// Return the absolute path to the directory containing migration folders
    ///
    /// The location returned is dependent on whether an absolute or relative path
    /// was provided to `migration_location` in either a settings file or settings builder.
    /// If an absolute path was provided, that same path is returned.
    /// If a relative path was provided, the path returned will be relative
    /// to either the settings file's directory if a settings file exists, or
    /// the current directory.
    pub fn migration_location(&self) -> Result<PathBuf> {
        let path = self
            .settings
            .inner
            .migration_location()
            .unwrap_or_else(|| PathBuf::from("migrations"));
        if path.is_absolute() {
            Ok(path)
        } else {
            Ok(self.relative_base_dir()?.join(path))
        }
    }

    /// The directory that relative settings paths are resolved against:
    /// the settings file's directory if one exists, the current directory otherwise
    fn relative_base_dir(&self) -> Result<PathBuf> {
        match self.settings_path.as_ref() {
            Some(settings_path) => Ok(settings_path
                .parent()
                .ok_or_else(|| {
                    err!(
                        PathError,
                        "Unable to determine parent path: {:?}",
                        settings_path
                    )
                })?
                .to_owned()),
            None => Ok(env::current_dir()?),
        }
    }

    /// Return the database type
    pub fn database_type(&self) -> DbKind {
        self.settings.inner.db_kind()
    }

    pub(crate) fn database_path_string(&self) -> Result<String> {
        let path = self.database_path()?;
        path.to_str()
            .map(str::to_owned)
            .ok_or_else(|| err!(PathError, "Invalid utf8 path: {:?}", path))
    }

    /// Return the absolute path to the database file. This is intended for
    /// sqlite databases only. In-memory sqlite databases return `:memory:`.
    pub fn database_path(&self) -> Result<PathBuf> {
        if self.settings.inner.is_memory_sqlite() {
            return Ok(PathBuf::from(SQLITE_MEMORY_PATH));
        }
        let path = self.settings.inner.database_path()?;
        if path.is_absolute() {
            Ok(path)
        } else {
            match self.settings_path.as_ref() {
                None => bail!(Config, "Settings path not specified"),
                Some(_) => Ok(self.relative_base_dir()?.join(path)),
            }
        }
    }

    /// Generate a database connection string.
    /// Not intended for file-based databases (sqlite)
    pub fn connect_string(&self) -> Result<String> {
        self.settings.inner.connect_string()
    }

    /// Return the custom ssl cert file, if any (postgres only)
    pub fn ssl_cert_file(&self) -> Option<PathBuf> {
        self.settings.inner.ssl_cert_file()
    }
}

/// Create a file (and any missing parent directories) if it doesn't exist,
/// returning `true` if the file was created
fn create_file_if_missing(path: &Path) -> Result<bool> {
    if path.exists() {
        return Ok(false);
    }
    let db_dir = path
        .parent()
        .ok_or_else(|| err!(PathError, "Unable to determine parent path: {:?}", path))?;
    fs::create_dir_all(db_dir).map_err(|e| {
        err!(
            Config,
            "Failed creating database directory {:?}: {}",
            db_dir,
            e
        )
    })?;
    fs::File::create(path)
        .map_err(|e| err!(Config, "Failed creating database file {:?}: {}", path, e))?;
    Ok(true)
}
