use std::io::Read;
use std::path::{Path, PathBuf};
use std::env;
use std::fs;
use std::collections::{HashSet, HashMap};

use toml;
use url;
use chrono::{self, TimeZone};

use drivers;
use {
    Migratable, encode, prompt, open_file_in_fg, write_to_path, invalid_tag, DbKind,
    FULL_TAG_RE, DT_FORMAT, CONFIG_FILE,
    PG_CONFIG_TEMPLATE, SQLITE_CONFIG_TEMPLATE,
};
use errors::*;


#[derive(Debug, Clone)]
/// Project configuration/settings builder to initialize a new config file
pub struct ConfigInitializer {
    dir: PathBuf,
    database_type: Option<String>,
    interactive: bool,
    database_name: Option<String>,
}
impl ConfigInitializer {
    /// Start a new `ConfigInitializer`
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_owned(),
            database_type: None,
            interactive: true,
            database_name: None,
        }
    }

    /// Specify the database_type, checks whether the type is supported
    pub fn for_database(mut self, db_type: Option<&str>) -> Result<Self> {
        match db_type {
            None => self.database_type = None,
            Some(db_type) => {
                match db_type {
                    "postgres" | "sqlite" => (),
                    e => bail_fmt!(ErrorKind::Config, "unsupported database type: {}", e),
                };
                self.database_type = Some(db_type.to_owned());
            }
        }
        Ok(self)
    }

    /// Set interactive prompts, default is `true`
    pub fn interactive(mut self, b: bool) -> Self {
        self.interactive = b;
        self
    }

    /// Determines whether new .migrant file location should be in
    /// the given directory or a user specified path
    fn confirm_new_config_location(dir: &Path) -> Result<PathBuf> {
        println!(" A new `{}` config file will be created at the following location: ", CONFIG_FILE);
        println!("   {:?}", dir.display());
        let ans = prompt(" Is this ok? [Y/n] ")?;
        if ans.is_empty() || ans.to_lowercase() == "y" {
            return Ok(dir.to_owned());
        }

        println!(" You can specify the absolute location now, or nothing to exit");
        let ans = prompt(" >> ")?;
        if ans.is_empty() {
            bail_fmt!(ErrorKind::Config, "No `{}` path provided", CONFIG_FILE)
        }

        let path = PathBuf::from(ans);
        if !path.is_absolute() || path.file_name().unwrap() != CONFIG_FILE {
            bail_fmt!(ErrorKind::Config, "Invalid absolute path: {}, must end in `{}`", path.display(), CONFIG_FILE);
        }
        Ok(path)
    }

    /// Specify database name to pre-populate config file with
    pub fn database_name(mut self, name: &str) -> Self {
        self.database_name = Some(name.to_string());
        self
    }

    /// Generate a template config file using provided parameters or prompting the user.
    /// If running interactively, the file will be opened for editing and `Config::setup`
    /// will be run automatically.
    pub fn initialize(self) -> Result<()> {
        let config_path = self.dir.join(CONFIG_FILE);
        let config_path = if !self.interactive {
            config_path
        } else {
            ConfigInitializer::confirm_new_config_location(&config_path)
                .map_err(|e| format_err!(ErrorKind::Config, "unable to create a `{}` config -> {}", CONFIG_FILE, e))?
        };

        let db_type = if let Some(db_type) = self.database_type.as_ref() {
            db_type.to_owned()
        } else {
            if !self.interactive {
                bail_fmt!(ErrorKind::Config, "database type must be specified if running non-interactively")
            }
            println!("\n ** Gathering database information...");
            let db_type = prompt(" database type (sqlite|postgres) >> ")?;
            match db_type.as_ref() {
                "postgres" | "sqlite" => (),
                e => bail_fmt!(ErrorKind::Config, "unsupported database type: {}", e),
            };
            db_type
        };

        println!("\n ** Writing {} config template to {:?}", db_type, config_path);
        match db_type.as_ref() {
            "postgres" => {
                let content = PG_CONFIG_TEMPLATE
                    .replace("__DB_NAME__", &self.database_name.unwrap_or_else(|| String::new()));
                write_to_path(&config_path, content.as_bytes())?;
            }
            "sqlite" => {
                let content = SQLITE_CONFIG_TEMPLATE
                    .replace("__CONFIG_DIR__", config_path.parent().unwrap().to_str().unwrap())
                    .replace("__DB_PATH__", &self.database_name.unwrap_or_else(|| String::new()));
                write_to_path(&config_path, content.as_bytes())?;
            }
            _ => unreachable!(),
        };

        println!("\n ** Please update `{}` with your database credentials and run `setup`", CONFIG_FILE);

        if self.interactive {
            let editor = env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
            let file_path = config_path.to_str().unwrap();
            let command = format!("{} {}", editor, file_path);
            println!(" -- Your config file will be opened with the following command: `{}`", &command);
            println!(" -- After editing, the `setup` command will be run for you");
            let _ = prompt(&format!(" -- Press [ENTER] to open now or [CTRL+C] to exit and edit manually"))?;
            open_file_in_fg(&editor, file_path)
                .map_err(|e| format_err!(ErrorKind::Config, "Error editing config file: {}", e))?;

            println!();
            let config = Config::from_settings_file(&config_path)?;
            let _setup = config.setup()?;
        }
        Ok(())
    }
}


#[derive(Deserialize, Debug, Clone)]
/// Project settings
///
/// These settings are serialized and saved in a project `Migrant.toml` config file
/// or defined explicitly in source using the provided builder methods.
pub struct Settings {
    pub(crate) database_type: String,
    pub(crate) migration_location: Option<String>,
    pub(crate) database_path: Option<String>,
    pub(crate) database_name: Option<String>,
    pub(crate) database_host: Option<String>,
    pub(crate) database_port: Option<String>,
    pub(crate) database_user: Option<String>,
    pub(crate) database_password: Option<String>,
    pub(crate) database_params: Option<HashMap<String, String>>,
}
impl Settings {
    /// Initialize from a serialized settings file
    pub fn from_file<T: AsRef<Path>>(path: T) -> Result<Self> {
        let mut f = fs::File::open(path.as_ref())?;
        let mut content = String::new();
        f.read_to_string(&mut content)?;
        let settings = toml::from_str::<Settings>(&content)?;
        Ok(settings)
    }

    /// Initialize an empty `Settings` to be configured
    pub fn with_db_type(db_type: DbKind) -> Self {
        Self {
            database_type: db_type.to_string(),
            migration_location: None,
            database_path: None,
            database_name: None,
            database_host: None,
            database_port: None,
            database_user: None,
            database_password: None,
            database_params: None,
        }
    }

    /// Set directory to look for migration files.
    pub fn migration_location<T: AsRef<Path>>(&mut self, p: T) -> Result<&mut Self> {
        let p = p.as_ref();
        let s = p.to_str().ok_or_else(|| format_err!(ErrorKind::PathError, "Unicode path error: {:?}", p))?;
        self.migration_location = Some(s.to_owned());
        Ok(self)
    }

    /// Set the path to look for a database file. Note, this is only used for sqlite
    /// and is the only property that is used/required for connecting to sqlite databases.
    pub fn database_path<T: AsRef<Path>>(&mut self, p: T) -> Result<&mut Self> {
        let p = p.as_ref();
        if ! p.is_absolute() { bail_fmt!(ErrorKind::Config, "Explicit settings database path must be absolute: {:?}", p) }
        let s = p.to_str().ok_or_else(|| format_err!(ErrorKind::PathError, "Unicode path error: {:?}", p))?;
        self.database_path = Some(s.to_owned());
        Ok(self)
    }

    /// Set the database name.
    pub fn database_name(&mut self, name: &str) -> &mut Self {
        self.database_name = Some(name.into());
        self
    }

    /// Set the database host.
    pub fn database_host(&mut self, host: &str) -> &mut Self {
        self.database_host = Some(host.into());
        self
    }

    /// Set the database port.
    pub fn database_port(&mut self, port: u16) -> &mut Self {
        self.database_port = Some(port.to_string());
        self
    }

    /// Set the database user.
    pub fn database_user(&mut self, user: &str) -> &mut Self {
        self.database_user = Some(user.into());
        self
    }

    /// Set the database password.
    pub fn database_password(&mut self, pass: &str) -> &mut Self {
        self.database_password = Some(pass.into());
        self
    }

    /// Set a collection of database connection parameters.
    pub fn database_params(&mut self, params: &[(&str, &str)]) -> &mut Self {
        let mut map = HashMap::new();
        for &(k, v) in params.iter() {
            map.insert(k.to_string(), v.to_string());
        }
        self.database_params = Some(map);
        self
    }
}


#[derive(Debug, Clone)]
/// Full project configuration
pub struct Config {
    pub(crate) settings: Settings,
    pub(crate) settings_path: Option<PathBuf>,
    pub(crate) applied: Vec<String>,
    pub(crate) migrations: Option<Vec<Box<Migratable>>>,
}
impl Config {
    /// Define an explicit set of `Migratable` migrations to use.
    ///
    /// When using explicit migrations, make sure they are defined on the `Config`
    /// instance before applied migrations are loaded from the database. This is
    /// required because tag format requirements are stricter for implicit
    /// (file-system based) migrations, requiring a timestamp to
    /// maintain a deterministic order.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// extern crate migrant_lib;
    /// use migrant_lib::{
    ///     Config, search_for_settings_file,
    ///     EmbeddedMigration, FileMigration, FnMigration
    /// };
    ///
    /// # fn run() -> Result<(), Box<std::error::Error>> {
    /// mod migrations {
    ///     use super::*;
    ///     pub struct Custom;
    ///     impl Custom {
    ///         pub fn up(_: migrant_lib::DbConn) -> Result<(), Box<std::error::Error>> {
    ///             print!(" <[Up!]>");
    ///             Ok(())
    ///         }
    ///         pub fn down(_: migrant_lib::DbConn) -> Result<(), Box<std::error::Error>> {
    ///             print!(" <[Down!]>");
    ///             Ok(())
    ///         }
    ///     }
    /// }
    ///
    /// let p = search_for_settings_file(&std::env::current_dir()?)
    ///     .ok_or_else(|| "Settings file not found")?;
    /// let mut config = Config::from_settings_file(&p)?;
    /// config.use_migrations(vec![
    ///     EmbeddedMigration::with_tag("initial")?
    ///         .up(include_str!("../migrations/initial/up.sql"))
    ///         .down(include_str!("../migrations/initial/down.sql"))
    ///         .boxed(),
    ///     FileMigration::with_tag("second")?
    ///         .up("migrations/second/up.sql")?
    ///         .down("migrations/second/down.sql")?
    ///         .boxed(),
    ///     FnMigration::with_tag("custom")?
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
    pub fn use_migrations(&mut self, migrations: Vec<Box<Migratable>>) -> Result<&mut Self> {
        let mut set = HashSet::new();
        for mig in &migrations {
            let tag = mig.tag();
            if set.contains(&tag) {
                bail_fmt!(ErrorKind::TagError, "Tags must be unique. Found duplicate: {}", tag)
            }
            set.insert(tag);
        }
        self.migrations = Some(migrations);
        Ok(self)
    }

    /// Migrations are explicitly defined
    pub fn is_explicit(&self) -> bool {
        self.migrations.is_some()
    }

    /// Check that migration tags conform to naming requirements.
    /// If migrations are explicitly defined (with `use_migrations`), then
    /// tags may only contain [a-z0-9-]. If migrations are managed by `migrant`,
    /// not specified with `use_migrations` and instead created by `migrant_lib::new`,
    /// then they must follow [0-9]{14}_[a-z0-9-] (<timestamp>_<name>).
    fn check_saved_tag(&self, tag: &str) -> Result<()> {
        if self.is_explicit() {
            if invalid_tag(tag) {
                bail_fmt!(ErrorKind::Migration, "Found a non-conforming tag in the database: `{}`. \
                                                 Managed tags may contain [a-z0-9-]", tag)
            }
        } else if !FULL_TAG_RE.is_match(&tag) {
            bail_fmt!(ErrorKind::Migration, "Found a non-conforming tag in the database: `{}`. \
                                             Generated tags must follow [0-9]{{14}}_[a-z0-9-]", tag)
        }
        Ok(())
    }

    /// Do a full reload of the configuration file (only if a settings file is being used) and
    /// query the database to load applied migrations, keeping track of
    /// manually specified `migrations`.
    pub fn reload(&self) -> Result<Config> {
        let mut config = match self.settings_path.as_ref() {
            Some(path) => Config::from_settings_file(path)?,
            None => self.clone(),
        };
        config.migrations = self.migrations.clone();
        let applied = config.load_applied()?;
        config.applied = applied;
        Ok(config)
    }

    /// Load config file from the given path without querying the database
    /// to check for applied migrations
    pub fn from_settings_file<T: AsRef<Path>>(path: T) -> Result<Config> {
        let path = path.as_ref();
        let settings = Settings::from_file(path)?;
        Ok(Config {
            settings_path: Some(path.to_owned()),
            settings: settings,
            applied: vec![],
            migrations: None,
        })
    }

    /// Initialize a `Config` using an explicitly created `Settings` object.
    /// This alleviates the need for a settings file.
    ///
    /// ```rust,no_run
    /// # extern crate migrant_lib;
    /// # use migrant_lib::{Settings, Config, DbKind};
    /// # fn main() { run().unwrap(); }
    /// # fn run() -> Result<(), Box<std::error::Error>> {
    /// let mut settings = Settings::with_db_type(DbKind::Sqlite);
    /// settings.database_path("/absolute/path/to/db.db")?;
    /// settings.migration_location("/absolute/path/to/migration_dir")?;
    /// let config = Config::with_settings(&settings);
    /// // Setup migrations table
    /// config.setup()?;
    ///
    /// // Reload config, ping the database for applied migrations
    /// let config = config.reload()?;
    /// # let _ = config;
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_settings(s: &Settings) -> Config {
        Config {
            settings: s.clone(),
            settings_path: None,
            applied: vec![],
            migrations: None,
        }
    }

    /// Load the applied migrations from the database migration table
    pub(crate) fn load_applied(&self) -> Result<Vec<String>> {
        if !self.migration_table_exists()? {
            bail_fmt!(ErrorKind::Migration, "`__migrant_migrations` table is missing, maybe try re-setting-up? -> `setup`")
        }

        let applied = match self.settings.database_type.as_ref() {
            "sqlite"    => drivers::sqlite::select_migrations(&self.database_path_string()?)?,
            "postgres"  => drivers::pg::select_migrations(&self.connect_string()?)?,
            _ => unreachable!(),
        };
        let mut tags = vec![];
        for tag in applied.into_iter() {
            self.check_saved_tag(&tag)?;
            tags.push(tag);
        }
        let tags = if self.is_explicit() { tags } else {
            let mut stamped = tags.into_iter().map(|tag| {
                let stamp = tag.split('_').next()
                    .ok_or_else(|| format_err!(ErrorKind::TagError, "Invalid tag format: {:?}", tag))?;
                let stamp = chrono::Utc.datetime_from_str(stamp, DT_FORMAT)?;
                Ok((stamp, tag.clone()))
            }).collect::<Result<Vec<_>>>()?;
            stamped.sort_by(|a, b| a.0.cmp(&b.0));
            stamped.into_iter().map(|tup| tup.1).collect::<Vec<_>>()
        };
        Ok(tags)
    }


    /// Check if a __migrant_migrations table exists
    pub(crate) fn migration_table_exists(&self) -> Result<bool> {
        match self.settings.database_type.as_ref() {
            "sqlite"    => drivers::sqlite::migration_table_exists(&self.database_path_string()?),
            "postgres"  => drivers::pg::migration_table_exists(&self.connect_string()?),
            _ => unreachable!()
        }
    }

    /// Insert given tag into database migration table
    pub(crate) fn insert_migration_tag(&self, tag: &str) -> Result<()> {
        match self.settings.database_type.as_ref() {
            "sqlite"    => drivers::sqlite::insert_migration_tag(&self.database_path_string()?, tag)?,
            "postgres"  => drivers::pg::insert_migration_tag(&self.connect_string()?, tag)?,
            _ => unreachable!(),
        };
        Ok(())
    }

    /// Remove a given tag from the database migration table
    pub(crate) fn delete_migration_tag(&self, tag: &str) -> Result<()> {
        match self.settings.database_type.as_ref() {
            "sqlite"    => drivers::sqlite::remove_migration_tag(&self.database_path_string()?, tag)?,
            "postgres"  => drivers::pg::remove_migration_tag(&self.connect_string()?, tag)?,
            _ => unreachable!(),
        };
        Ok(())
    }

    /// Start a config initializer in the given directory
    pub fn init_in(dir: &Path) -> ConfigInitializer {
        ConfigInitializer::new(dir)
    }

    /// - Confirm the database can be accessed
    /// - Setup the database migrations table if it doesn't exist yet
    pub fn setup(&self) -> Result<bool> {
        debug!(" ** Confirming database credentials...");
        match self.settings.database_type.as_ref() {
            "sqlite" => {
                if self.settings.database_path.is_none() {
                    bail_fmt!(ErrorKind::Config, "`database_path` is required!")
                }
                let db_path = self.database_path()?;
                let created = drivers::sqlite::create_file_if_missing(&db_path)?;
                debug!("    - checking if db file already exists...");
                if created {
                    debug!("    - db not found... creating now... ✓")
                } else {
                    debug!("    - db already exists ✓");
                }
            }
            "postgres" => {
                let conn_str = self.connect_string()?;
                let can_connect = drivers::pg::can_connect(&conn_str)?;
                if !can_connect {
                    debug!(" ERROR: Unable to connect to {}", conn_str);
                    debug!("        Please initialize your database and user and then run `setup`");
                    debug!("\n  ex) sudo -u postgres createdb {}", self.settings.database_name.as_ref().unwrap());
                    debug!("      sudo -u postgres createuser {}", self.settings.database_user.as_ref().unwrap());
                    debug!("      sudo -u postgres psql -c \"alter user {} with password '****'\"", self.settings.database_user.as_ref().unwrap());
                    debug!("");
                    bail_fmt!(ErrorKind::Config,
                              "Cannot connect to postgres database with connection string: {:?}. \
                               Do the database & user exist?",
                              conn_str);
                } else {
                    debug!("    - Connection confirmed ✓");
                }
            }
            _ => unreachable!(),
        }

        debug!("\n ** Setting up migrations table");
        let table_created = match self.settings.database_type.as_ref() {
            "sqlite" => {
                let db_path = self.database_path()?;
                drivers::sqlite::migration_setup(&db_path)?
            }
            "postgres" => {
                let conn_str = self.connect_string()?;
                drivers::pg::migration_setup(&conn_str)?
            }
            _ => unreachable!(),
        };

        if table_created {
            debug!("    - migrations table missing");
            debug!("    - `__migrant_migrations` table created ✓");
            Ok(true)
        } else {
            debug!("    - `__migrant_migrations` table already exists ✓");
            Ok(false)
        }
    }

    /// Return the absolute path to the directory containing migration folders
    pub fn migration_dir(&self) -> Result<PathBuf> {
        let path = Path::new(self.settings.migration_location.as_ref()
                             .ok_or_else(|| format_err!(ErrorKind::Config, "Migration location not specified"))?);
        Ok(if path.is_absolute() { path.to_owned() } else {
            let spath = Path::new(self.settings_path.as_ref()
                                  .ok_or_else(|| format_err!(ErrorKind::Config, "Settings path not specified"))?);
            let spath = spath.parent()
                .ok_or_else(|| format_err!(ErrorKind::PathError, "Unable to determine parent path: {:?}", spath))?;
            spath.join(path)
        })
    }

    /// Return the database type
    pub fn database_type(&self) -> Result<String> {
        Ok(self.settings.database_type.to_owned())
    }

    fn database_path_string(&self) -> Result<String> {
        let path = self.database_path()?;
        let path = path.to_str()
            .ok_or_else(|| format_err!(ErrorKind::PathError, "Invalid utf8 path: {:?}", path))?
            .to_owned();
        Ok(path)
    }

    /// Return the absolute path to the database file. This is intended for
    /// sqlite3 databases only
    pub fn database_path(&self) -> Result<PathBuf> {
        Ok(match self.settings.database_type.as_ref() {
            "sqlite" => {
                let path = Path::new(self.settings.database_path.as_ref()
                                     .ok_or_else(|| format_err!(ErrorKind::Config, "Database path not specified"))?);
                if path.is_absolute() { path.to_owned() } else {
                    let spath = Path::new(self.settings_path.as_ref()
                                          .ok_or_else(|| format_err!(ErrorKind::Config, "Settings path not specified"))?);
                    let spath = spath.parent()
                        .ok_or_else(|| format_err!(ErrorKind::PathError, "Unable to determine parent path: {:?}", spath))?;
                    spath.join(path)
                }

            }
            db_t => bail_fmt!(ErrorKind::Config, "Cannot retrieve database-path for database-type: {}", db_t),
        })
    }

    /// Generate a database connection string.
    /// Not intended for file-based databases (sqlite)
    pub fn connect_string(&self) -> Result<String> {
        match self.settings.database_type.as_ref() {
            "postgres" => (),
            db_t => bail_fmt!(ErrorKind::Config, "Cannot generate connect-string for database-type: {}", db_t),
        };

        let pass = match self.settings.database_password {
            Some(ref pass) => {
                let p = encode(pass);
                format!(":{}", p)
            },
            None => "".into(),
        };
        let user = match self.settings.database_user.as_ref().and_then(|s| if s.is_empty() { None } else { Some(s) }) {
            Some(ref user) => encode(user),
            None => bail_fmt!(ErrorKind::Config, "'database_user' not specified"),
        };

        let db_name = match self.settings.database_name.as_ref().and_then(|s| if s.is_empty() { None } else { Some(s) }) {
            Some(ref name) => encode(name),
            None => bail_fmt!(ErrorKind::Config, "`database_name` not specified"),
        };

        let host = self.settings.database_host.clone().unwrap_or_else(|| "localhost".to_string());
        let host = if host.is_empty() { "localhost".to_string() } else { host };
        let host = encode(&host);

        let port = self.settings.database_port.clone().unwrap_or_else(|| "5432".to_string());
        let port = if host.is_empty() { "5432".to_string() } else { port };
        let port = encode(&port);

        let s = format!("{db_type}://{user}{pass}@{host}:{port}/{db_name}",
                db_type=self.settings.database_type,
                user=user,
                pass=pass,
                host=host,
                port=port,
                db_name=db_name);

        let mut url = url::Url::parse(&s)?;

        if let Some(ref params) = self.settings.database_params {
            let mut pairs = vec![];
            for (k, v) in params.iter() {
                let k = encode(k);
                let v = encode(v);
                pairs.push((k, v));
            }
            if !pairs.is_empty() {
                let mut url = url.query_pairs_mut();
                for &(ref k, ref v) in &pairs {
                    url.append_pair(k, v);
                }
            }
        }

        Ok(url.into_string())
    }
}

