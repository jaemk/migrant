/*!
Settings file initialization
*/
use std::env;
use std::path::{Path, PathBuf};

use crate::errors::*;
use crate::macros::{bail, err};
use crate::util::{open_file_in_fg, prompt, write_to_path};
use crate::{DbKind, CONFIG_FILE};

use super::builders::{
    MySqlSettingsBuilder, PostgresSettingsBuilder, ServerSettingsBuilder, SqliteSettingsBuilder,
};
use super::Config;

static SQLITE_CONFIG_TEMPLATE: &str = r#"
# Required, do not edit
database_type = "sqlite"

# Required: Absolute or relative path to your database file.
#           If a relative path is provided, it will be assumed
#           to be relative to this config file dir: `__CONFIG_DIR__/`
# ex.) database_name = "db/db.db"
database_path = "__DB_PATH__"

migration_location = "__MIG_LOC__"  # default "migrations"

"#;

static PG_CONFIG_TEMPLATE: &str = r#"
# Required, do not edit
database_type = "postgres"

# Required database info
database_name = "__DB_NAME__"
database_user = "__DB_USER__"
database_password = "__DB_PASS__"

# Configurable database info
database_host = "__DB_HOST__"         # default "localhost"
database_port = "__DB_PORT__"              # default "5432"
migration_location = "__MIG_LOC__"  # default "migrations"

# Optional customer ssl cert file
# ssl_cert_file = "path/to/certificate.crt.pem.key"

# Extra database connection parameters
# with the format:
# [database_params]
# key = "value"
[database_params]
"#;

static MYSQL_CONFIG_TEMPLATE: &str = r#"
# Required, do not edit
database_type = "mysql"

# Required database info
database_name = "__DB_NAME__"
database_user = "__DB_USER__"
database_password = "__DB_PASS__"

# Configurable database info
database_host = "__DB_HOST__"         # default "localhost"
database_port = "__DB_PORT__"              # default "3306"
migration_location = "__MIG_LOC__"  # default "migrations"

# Extra database connection parameters
# with the format:
# [database_params]
# key = "value"
[database_params]
"#;

#[derive(Debug, Clone)]
enum DatabaseConfigOptions {
    Sqlite(SqliteSettingsBuilder),
    Postgres(PostgresSettingsBuilder),
    MySql(MySqlSettingsBuilder),
}

/// Project settings file builder to initialize a new settings file
#[derive(Debug, Clone)]
pub struct SettingsFileInitializer {
    dir: PathBuf,
    interactive: bool,
    with_env_defaults: bool,
    database_options: Option<DatabaseConfigOptions>,
}

/// Template value resolution: explicit value, `env:VAR` placeholder, or fallback
fn value_or(explicit: Option<&str>, with_env: bool, env_var: &str, fallback: &str) -> String {
    match explicit {
        Some(v) => v.to_string(),
        None if with_env => format!("env:{}", env_var),
        None => fallback.to_string(),
    }
}

/// Resolve an optional path option to its UTF-8 string form for template
/// substitution.
///
/// An explicitly-provided non-UTF-8 path is rejected with a `PathError`, mirroring
/// the `SqliteSettingsBuilder::build()` / `ServerSettingsBuilder::build()`
/// validation so `init` and `build()` agree. A genuinely-absent option yields
/// `None`, so the normal env/fallback default still applies.
fn path_opt_str(opt: Option<&Path>) -> Result<Option<&str>> {
    opt.map(|p| {
        p.to_str()
            .ok_or_else(|| err!(PathError, "Unicode path error: {:?}", p))
    })
    .transpose()
}

/// Render the pg/mysql shared template values
fn render_server_template(
    template: &str,
    opts: &ServerSettingsBuilder,
    with_env: bool,
    default_port: &str,
) -> Result<String> {
    let mut content = template
        .replace(
            "__DB_NAME__",
            &value_or(opts.database_name.as_deref(), with_env, "DATABASE_NAME", ""),
        )
        .replace(
            "__DB_USER__",
            &value_or(opts.database_user.as_deref(), with_env, "DATABASE_USER", ""),
        )
        .replace(
            "__DB_PASS__",
            &value_or(
                opts.database_password.as_deref(),
                with_env,
                "DATABASE_PASSWORD",
                "",
            ),
        )
        .replace(
            "__DB_HOST__",
            &value_or(
                opts.database_host.as_deref(),
                with_env,
                "DATABASE_HOST",
                "localhost",
            ),
        )
        .replace(
            "__DB_PORT__",
            &value_or(
                opts.database_port.as_deref(),
                with_env,
                "DATABASE_PORT",
                default_port,
            ),
        )
        .replace(
            "__MIG_LOC__",
            &value_or(
                path_opt_str(opts.migration_location.as_deref())?,
                with_env,
                "MIGRATION_LOCATION",
                "migrations",
            ),
        );
    match opts.database_params {
        Some(ref params) => {
            for (k, v) in params.iter() {
                content.push_str(&format!("{} = {:?}\n", k, v));
            }
        }
        None => content.push('\n'),
    }
    content.push('\n');
    Ok(content)
}

impl SettingsFileInitializer {
    pub(crate) fn new<T: AsRef<Path>>(dir: T) -> Self {
        Self {
            dir: dir.as_ref().to_owned(),
            interactive: true,
            with_env_defaults: false,
            database_options: None,
        }
    }

    /// Set interactive prompts, default is `true`
    pub fn interactive(mut self, b: bool) -> Self {
        self.interactive = b;
        self
    }

    /// Default all file values `env:<ENV_VAR>` if unspecified
    pub fn with_env_defaults(mut self, b: bool) -> Self {
        self.with_env_defaults = b;
        self
    }

    /// Specify Sqlite database options
    ///
    /// ## Example:
    ///
    /// ```rust,no_run
    /// # use std::env;
    /// use migrant_lib::Config;
    /// use migrant_lib::config::SqliteSettingsBuilder;
    /// # fn main() { run().unwrap() }
    /// # fn run() -> Result<(), Box<dyn std::error::Error>> {
    /// Config::init_in(env::current_dir()?)
    ///     .with_sqlite_options(
    ///         SqliteSettingsBuilder::empty()
    ///             .database_path("/abs/path/to/my.db"))
    ///     .initialize()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_sqlite_options(mut self, options: SqliteSettingsBuilder) -> Self {
        self.database_options = Some(DatabaseConfigOptions::Sqlite(options));
        self
    }

    /// Specify Postgres database options
    ///
    /// ## Example:
    ///
    /// ```rust,no_run
    /// # use std::env;
    /// use migrant_lib::Config;
    /// use migrant_lib::config::PostgresSettingsBuilder;
    /// # fn main() { run().unwrap() }
    /// # fn run() -> Result<(), Box<dyn std::error::Error>> {
    /// Config::init_in(env::current_dir()?)
    ///     .with_postgres_options(
    ///         PostgresSettingsBuilder::empty()
    ///             .database_name("my_db")
    ///             .database_user("me")
    ///             .database_port(4444))
    ///     .initialize()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_postgres_options(mut self, options: PostgresSettingsBuilder) -> Self {
        self.database_options = Some(DatabaseConfigOptions::Postgres(options));
        self
    }

    /// Specify MySQL database options
    ///
    /// ## Example:
    ///
    /// ```rust,no_run
    /// # use std::env;
    /// use migrant_lib::Config;
    /// use migrant_lib::config::MySqlSettingsBuilder;
    /// # fn main() { run().unwrap() }
    /// # fn run() -> Result<(), Box<dyn std::error::Error>> {
    /// Config::init_in(env::current_dir()?)
    ///     .with_mysql_options(
    ///         MySqlSettingsBuilder::empty()
    ///             .database_name("my_db")
    ///             .database_user("me")
    ///             .database_port(4444))
    ///     .initialize()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_mysql_options(mut self, options: MySqlSettingsBuilder) -> Self {
        self.database_options = Some(DatabaseConfigOptions::MySql(options));
        self
    }

    /// Determines whether the new settings file location should be in
    /// the given directory or a user specified path
    fn confirm_new_config_location(dir: &Path) -> Result<PathBuf> {
        println!(
            " A new `{}` config file will be created at the following location: ",
            CONFIG_FILE
        );
        println!("   {:?}", dir.display());
        let ans = prompt(" Is this ok? [Y/n] ")?;
        if ans.is_empty() || ans.to_lowercase() == "y" {
            return Ok(dir.to_owned());
        }

        println!(" You can specify the absolute location now, or nothing to exit");
        let ans = prompt(" >> ")?;
        if ans.is_empty() {
            bail!(Config, "No `{}` path provided", CONFIG_FILE)
        }

        let path = PathBuf::from(ans);
        if !path.is_absolute() || path.file_name().is_none_or(|n| n != CONFIG_FILE) {
            bail!(
                Config,
                "Invalid absolute path: {}, must end in `{}`",
                path.display(),
                CONFIG_FILE
            );
        }
        Ok(path)
    }

    /// Prompt the user for a database type, returning default options for it
    fn prompt_for_database_options() -> Result<DatabaseConfigOptions> {
        println!("\n ** Gathering database information...");
        let db_kind = prompt(" database type (sqlite|postgres|mysql) >> ")?;
        let db_kind = db_kind
            .parse::<DbKind>()
            .map_err(|_| err!(Config, "unsupported database type: {}", db_kind))?;
        Ok(match db_kind {
            DbKind::Sqlite => {
                let options = SqliteSettingsBuilder::empty().migration_location("migrations");
                DatabaseConfigOptions::Sqlite(options)
            }
            DbKind::Postgres => {
                let options = PostgresSettingsBuilder::empty().migration_location("migrations");
                DatabaseConfigOptions::Postgres(options)
            }
            DbKind::MySql => {
                let options = MySqlSettingsBuilder::empty().migration_location("migrations");
                DatabaseConfigOptions::MySql(options)
            }
        })
    }

    /// Generate a template config file using provided parameters or prompting the user.
    /// If running interactively, the file will be opened for editing and `Config::setup`
    /// will be run automatically.
    pub fn initialize(&self) -> Result<()> {
        let config_path = self.dir.join(CONFIG_FILE);
        let config_path = if !self.interactive {
            config_path
        } else {
            Self::confirm_new_config_location(&config_path).map_err(|e| {
                err!(
                    Config,
                    "unable to create a `{}` config -> {}",
                    CONFIG_FILE,
                    e
                )
            })?
        };

        let db_options = match self.database_options {
            Some(ref options) => options.clone(),
            None => {
                if !self.interactive {
                    bail!(
                        Config,
                        "database type must be specified if running non-interactively with options specified"
                    )
                }
                Self::prompt_for_database_options()?
            }
        };
        let db_kind = match db_options {
            DatabaseConfigOptions::Sqlite(_) => DbKind::Sqlite,
            DatabaseConfigOptions::Postgres(_) => DbKind::Postgres,
            DatabaseConfigOptions::MySql(_) => DbKind::MySql,
        };

        println!(
            "\n ** Writing {} config template to {:?}",
            db_kind, config_path
        );
        let content = match db_options {
            DatabaseConfigOptions::Postgres(ref opts) => render_server_template(
                PG_CONFIG_TEMPLATE,
                &opts.inner,
                self.with_env_defaults,
                "5432",
            )?,
            DatabaseConfigOptions::MySql(ref opts) => render_server_template(
                MYSQL_CONFIG_TEMPLATE,
                &opts.inner,
                self.with_env_defaults,
                "3306",
            )?,
            DatabaseConfigOptions::Sqlite(ref opts) => {
                let config_dir = config_path.parent().and_then(Path::to_str).ok_or_else(|| {
                    err!(
                        PathError,
                        "Unable to determine config dir: {:?}",
                        config_path
                    )
                })?;
                SQLITE_CONFIG_TEMPLATE
                    .replace("__CONFIG_DIR__", config_dir)
                    .replace(
                        "__DB_PATH__",
                        &value_or(
                            path_opt_str(opts.database_path.as_deref())?,
                            self.with_env_defaults,
                            "DATABASE_PATH",
                            "",
                        ),
                    )
                    .replace(
                        "__MIG_LOC__",
                        &value_or(
                            path_opt_str(opts.migration_location.as_deref())?,
                            self.with_env_defaults,
                            "MIGRATION_LOCATION",
                            "migrations",
                        ),
                    )
            }
        };
        write_to_path(&config_path, content.as_bytes())?;

        println!(
            "\n ** Please update `{}` with your database credentials and run `setup`\n",
            CONFIG_FILE
        );

        if self.interactive {
            let editor = env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
            let file_path = config_path
                .to_str()
                .ok_or_else(|| err!(PathError, "Invalid utf8 path: {:?}", config_path))?;
            let command = format!("{} {}", editor, file_path);
            println!(
                " -- Your config file will be opened with the following command: `{}`",
                command
            );
            println!(" -- After editing, the `setup` command will be run for you");
            let _ = prompt(" -- Press [ENTER] to open now or [CTRL+C] to exit and edit manually")?;
            open_file_in_fg(&editor, file_path)
                .map_err(|e| err!(Config, "Error editing config file: {}", e))?;

            println!();
            let config = Config::from_settings_file(&config_path)?;
            let _setup = config.setup()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The template renderer's `value_or(... Path::to_str ...)` resolution.
    #[test]
    fn value_or_resolves_explicit_env_and_fallback() {
        assert_eq!(value_or(Some("v"), false, "VAR", "fallback"), "v");
        // env-defaults on and no explicit value -> `env:VAR` placeholder
        assert_eq!(value_or(None, true, "VAR", "fallback"), "env:VAR");
        // no explicit value, no env-defaults -> the fallback
        assert_eq!(value_or(None, false, "VAR", "fallback"), "fallback");
        // an explicit value wins even with env-defaults on
        assert_eq!(value_or(Some("v"), true, "VAR", "fallback"), "v");
    }

    /// A non-UTF-8 path explicitly provided to sqlite `init` is rejected with a
    /// `PathError`, mirroring `SqliteSettingsBuilder::build()`, rather than
    /// silently falling back to a default. No settings file is written.
    #[cfg(unix)]
    #[test]
    fn sqlite_init_rejects_non_utf8_paths() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let bad = std::path::PathBuf::from(OsStr::from_bytes(&[0x2f, 0x66, 0x6f, 0x80, 0x6f]));
        let dir = tempfile::tempdir().unwrap();

        // A non-UTF-8 database_path is rejected.
        let err = SettingsFileInitializer::new(dir.path())
            .interactive(false)
            .with_sqlite_options(SqliteSettingsBuilder::empty().database_path(&bad))
            .initialize()
            .unwrap_err();
        assert!(
            matches!(err, Error::PathError(_)),
            "non-utf8 database_path must be rejected with a PathError, got {err:?}"
        );

        // A non-UTF-8 migration_location is rejected too (with a valid db path).
        let err = SettingsFileInitializer::new(dir.path())
            .interactive(false)
            .with_sqlite_options(
                SqliteSettingsBuilder::empty()
                    .database_path("/abs/db.db")
                    .migration_location(&bad),
            )
            .initialize()
            .unwrap_err();
        assert!(
            matches!(err, Error::PathError(_)),
            "non-utf8 migration_location must be rejected with a PathError, got {err:?}"
        );

        // The failed init did not leave a settings file behind.
        assert!(
            !dir.path().join(crate::CONFIG_FILE).exists(),
            "no settings file should be written when a path is rejected"
        );
    }

    /// The same rejection applies to the shared pg/mysql server template's
    /// `migration_location`.
    #[cfg(unix)]
    #[test]
    fn server_init_rejects_non_utf8_migration_location() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let bad = std::path::PathBuf::from(OsStr::from_bytes(&[0x2f, 0x66, 0x6f, 0x80, 0x6f]));
        let dir = tempfile::tempdir().unwrap();

        let err = SettingsFileInitializer::new(dir.path())
            .interactive(false)
            .with_postgres_options(
                PostgresSettingsBuilder::empty()
                    .database_name("db")
                    .database_user("me")
                    .database_password("secret")
                    .migration_location(&bad),
            )
            .initialize()
            .unwrap_err();
        assert!(
            matches!(err, Error::PathError(_)),
            "non-utf8 server migration_location must be rejected with a PathError, got {err:?}"
        );
        assert!(
            !dir.path().join(crate::CONFIG_FILE).exists(),
            "no settings file should be written when a path is rejected"
        );
    }

    /// Ordinary UTF-8 paths -- and omitted path options -- still render and write
    /// the settings file successfully, using explicit values where given and the
    /// normal defaults where absent.
    #[test]
    fn init_writes_template_for_utf8_and_omitted_paths() {
        // sqlite with explicit UTF-8 paths
        let dir = tempfile::tempdir().unwrap();
        SettingsFileInitializer::new(dir.path())
            .interactive(false)
            .with_sqlite_options(
                SqliteSettingsBuilder::empty()
                    .database_path("db/db.db")
                    .migration_location("migrations/managed"),
            )
            .initialize()
            .expect("utf8 sqlite init succeeds");
        let written = std::fs::read_to_string(dir.path().join(crate::CONFIG_FILE)).unwrap();
        assert!(
            written.contains(r#"database_path = "db/db.db""#),
            "{written}"
        );
        assert!(
            written.contains(r#"migration_location = "migrations/managed""#),
            "{written}"
        );

        // postgres with migration_location omitted -> normal default is used
        let dir = tempfile::tempdir().unwrap();
        SettingsFileInitializer::new(dir.path())
            .interactive(false)
            .with_postgres_options(
                PostgresSettingsBuilder::empty()
                    .database_name("db")
                    .database_user("me")
                    .database_password("secret"),
            )
            .initialize()
            .expect("postgres init with omitted migration_location succeeds");
        let written = std::fs::read_to_string(dir.path().join(crate::CONFIG_FILE)).unwrap();
        assert!(
            written.contains(r#"migration_location = "migrations""#),
            "omitted migration_location uses its default: {written}"
        );
    }
}
