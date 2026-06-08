/*!
Configuration structs
*/

use std::collections::{BTreeMap, HashSet};
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use chrono::{self, TimeZone};
use toml;
use url;

use crate::drivers;
use crate::errors::*;
use crate::{
    encode, invalid_full_tag, invalid_optional_stamp_tag, open_file_in_fg, prompt, write_to_path,
    DbKind, Migratable, CONFIG_FILE, DT_FORMAT, MYSQL_CONFIG_TEMPLATE, PG_CONFIG_TEMPLATE,
    SQLITE_CONFIG_TEMPLATE,
};

#[derive(Debug, Clone)]
enum DatabaseConfigOptions {
    Sqlite(SqliteSettingsBuilder),
    Postgres(PostgresSettingsBuilder),
    MySql(MySqlSettingsBuilder),
}

#[derive(Debug, Clone)]
/// Project settings file builder to initialize a new settings file
pub struct SettingsFileInitializer {
    dir: PathBuf,
    interactive: bool,
    with_env_defaults: bool,
    database_options: Option<DatabaseConfigOptions>,
}
impl SettingsFileInitializer {
    /// Start a new `ConfigInitializer`
    fn new<T: AsRef<Path>>(dir: T) -> Self {
        Self {
            dir: dir.as_ref().to_owned(),
            interactive: true,
            with_env_defaults: false,
            database_options: None,
        }
    }

    /// Set interactive prompts, default is `true`
    pub fn interactive(&mut self, b: bool) -> &mut Self {
        self.interactive = b;
        self
    }

    /// Default all file values `env:<ENV_VAR>` if unspecified
    pub fn with_env_defaults(&mut self, b: bool) -> &mut Self {
        self.with_env_defaults = b;
        self
    }

    /// Specify Sqlite database options
    ///
    /// ## Example:
    ///
    /// ```rust,no_run
    /// # extern crate migrant_lib;
    /// # use std::env;
    /// use migrant_lib::Config;
    /// use migrant_lib::config::SqliteSettingsBuilder;
    /// # fn main() { run().unwrap() }
    /// # fn run() -> Result<(), Box<dyn std::error::Error>> {
    /// Config::init_in(env::current_dir()?)
    ///     .with_sqlite_options(
    ///         SqliteSettingsBuilder::empty()
    ///             .database_path("/abs/path/to/my.db")?)
    ///     .initialize()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_sqlite_options(&mut self, options: &SqliteSettingsBuilder) -> &mut Self {
        self.database_options = Some(DatabaseConfigOptions::Sqlite(options.clone()));
        self
    }

    /// Specify Postgres database options
    ///
    /// ## Example:
    ///
    /// ```rust,no_run
    /// # extern crate migrant_lib;
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
    pub fn with_postgres_options(&mut self, options: &PostgresSettingsBuilder) -> &mut Self {
        self.database_options = Some(DatabaseConfigOptions::Postgres(options.clone()));
        self
    }

    /// Specify MySQL database options
    ///
    /// ## Example:
    ///
    /// ```rust,no_run
    /// # extern crate migrant_lib;
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
    pub fn with_mysql_options(&mut self, options: &MySqlSettingsBuilder) -> &mut Self {
        self.database_options = Some(DatabaseConfigOptions::MySql(options.clone()));
        self
    }

    /// Determines whether new .migrant file location should be in
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
            bail_fmt!(ErrorKind::Config, "No `{}` path provided", CONFIG_FILE)
        }

        let path = PathBuf::from(ans);
        if !path.is_absolute() || path.file_name().unwrap() != CONFIG_FILE {
            bail_fmt!(
                ErrorKind::Config,
                "Invalid absolute path: {}, must end in `{}`",
                path.display(),
                CONFIG_FILE
            );
        }
        Ok(path)
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
                format_err!(
                    ErrorKind::Config,
                    "unable to create a `{}` config -> {}",
                    CONFIG_FILE,
                    e
                )
            })?
        };

        let (db_kind, db_options) = if let Some(ref options) = self.database_options {
            let kind = match options {
                DatabaseConfigOptions::Sqlite(_) => DbKind::Sqlite,
                DatabaseConfigOptions::Postgres(_) => DbKind::Postgres,
                DatabaseConfigOptions::MySql(_) => DbKind::MySql,
            };
            (kind, options.clone())
        } else {
            if !self.interactive {
                bail_fmt!(ErrorKind::Config, "database type must be specified if running non-interactively with options specified")
            }
            println!("\n ** Gathering database information...");
            let db_kind = {
                let db_kind = prompt(" database type (sqlite|postgres|mysql) >> ")?;
                match db_kind.parse::<DbKind>() {
                    Ok(kind) => kind,
                    Err(_) => {
                        bail_fmt!(ErrorKind::Config, "unsupported database type: {}", db_kind)
                    }
                }
            };
            let options = match db_kind {
                DbKind::Sqlite => {
                    let mut options = SqliteSettingsBuilder::empty();
                    options.migration_location("migrations")?;
                    DatabaseConfigOptions::Sqlite(options)
                }
                DbKind::Postgres => {
                    let mut options = PostgresSettingsBuilder::empty();
                    options.migration_location("migrations")?;
                    DatabaseConfigOptions::Postgres(options)
                }
                DbKind::MySql => {
                    let mut options = MySqlSettingsBuilder::empty();
                    options.migration_location("migrations")?;
                    DatabaseConfigOptions::MySql(options)
                }
            };
            (db_kind, options)
        };

        println!(
            "\n ** Writing {} config template to {:?}",
            db_kind, config_path
        );
        match db_options {
            DatabaseConfigOptions::Postgres(ref opts) => {
                let mut content = PG_CONFIG_TEMPLATE
                    .replace(
                        "__DB_NAME__",
                        &opts.database_name.as_ref().cloned().unwrap_or_else(|| {
                            if self.with_env_defaults {
                                String::from("env:DATABASE_NAME")
                            } else {
                                String::new()
                            }
                        }),
                    )
                    .replace(
                        "__DB_USER__",
                        &opts.database_user.as_ref().cloned().unwrap_or_else(|| {
                            if self.with_env_defaults {
                                String::from("env:DATABASE_USER")
                            } else {
                                String::new()
                            }
                        }),
                    )
                    .replace(
                        "__DB_PASS__",
                        &opts.database_password.as_ref().cloned().unwrap_or_else(|| {
                            if self.with_env_defaults {
                                String::from("env:DATABASE_PASSWORD")
                            } else {
                                String::new()
                            }
                        }),
                    )
                    .replace(
                        "__DB_HOST__",
                        &opts.database_host.as_ref().cloned().unwrap_or_else(|| {
                            if self.with_env_defaults {
                                String::from("env:DATABASE_HOST")
                            } else {
                                String::from("localhost")
                            }
                        }),
                    )
                    .replace(
                        "__DB_PORT__",
                        &opts.database_port.as_ref().cloned().unwrap_or_else(|| {
                            if self.with_env_defaults {
                                String::from("env:DATABASE_PORT")
                            } else {
                                String::from("5432")
                            }
                        }),
                    )
                    .replace(
                        "__MIG_LOC__",
                        &opts
                            .migration_location
                            .as_ref()
                            .cloned()
                            .unwrap_or_else(|| {
                                if self.with_env_defaults {
                                    String::from("env:MIGRATION_LOCATION")
                                } else {
                                    String::from("migrations")
                                }
                            }),
                    );
                if let Some(ref params) = opts.database_params {
                    for (k, v) in params.iter() {
                        content.push_str(&format!("{} = {:?}\n", k, v));
                    }
                } else {
                    content.push('\n');
                }
                content.push('\n');
                write_to_path(&config_path, content.as_bytes())?;
            }
            DatabaseConfigOptions::MySql(ref opts) => {
                let mut content = MYSQL_CONFIG_TEMPLATE
                    .replace(
                        "__DB_NAME__",
                        &opts.database_name.as_ref().cloned().unwrap_or_else(|| {
                            if self.with_env_defaults {
                                String::from("env:DATABASE_NAME")
                            } else {
                                String::new()
                            }
                        }),
                    )
                    .replace(
                        "__DB_USER__",
                        &opts.database_user.as_ref().cloned().unwrap_or_else(|| {
                            if self.with_env_defaults {
                                String::from("env:DATABASE_USER")
                            } else {
                                String::new()
                            }
                        }),
                    )
                    .replace(
                        "__DB_PASS__",
                        &opts.database_password.as_ref().cloned().unwrap_or_else(|| {
                            if self.with_env_defaults {
                                String::from("env:DATABASE_PASSWORD")
                            } else {
                                String::new()
                            }
                        }),
                    )
                    .replace(
                        "__DB_HOST__",
                        &opts.database_host.as_ref().cloned().unwrap_or_else(|| {
                            if self.with_env_defaults {
                                String::from("env:DATABASE_HOST")
                            } else {
                                String::from("localhost")
                            }
                        }),
                    )
                    .replace(
                        "__DB_PORT__",
                        &opts.database_port.as_ref().cloned().unwrap_or_else(|| {
                            if self.with_env_defaults {
                                String::from("env:DATABASE_PORT")
                            } else {
                                String::from("3306")
                            }
                        }),
                    )
                    .replace(
                        "__MIG_LOC__",
                        &opts
                            .migration_location
                            .as_ref()
                            .cloned()
                            .unwrap_or_else(|| {
                                if self.with_env_defaults {
                                    String::from("env:MIGRATION_LOCATION")
                                } else {
                                    String::from("migrations")
                                }
                            }),
                    );
                if let Some(ref params) = opts.database_params {
                    for (k, v) in params.iter() {
                        content.push_str(&format!("{} = {:?}\n", k, v));
                    }
                } else {
                    content.push('\n');
                }
                content.push('\n');
                write_to_path(&config_path, content.as_bytes())?;
            }
            DatabaseConfigOptions::Sqlite(ref opts) => {
                let content = SQLITE_CONFIG_TEMPLATE
                    .replace(
                        "__CONFIG_DIR__",
                        config_path.parent().unwrap().to_str().unwrap(),
                    )
                    .replace(
                        "__DB_PATH__",
                        &opts.database_path.as_ref().cloned().unwrap_or_else(|| {
                            if self.with_env_defaults {
                                String::from("env:DATABASE_PATH")
                            } else {
                                String::new()
                            }
                        }),
                    )
                    .replace(
                        "__MIG_LOC__",
                        &opts
                            .migration_location
                            .as_ref()
                            .cloned()
                            .unwrap_or_else(|| {
                                if self.with_env_defaults {
                                    String::from("env:MIGRATION_LOCATION")
                                } else {
                                    String::from("migrations")
                                }
                            }),
                    );
                write_to_path(&config_path, content.as_bytes())?;
            }
        };

        println!(
            "\n ** Please update `{}` with your database credentials and run `setup`\n",
            CONFIG_FILE
        );

        if self.interactive {
            let editor = env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
            let file_path = config_path.to_str().unwrap();
            let command = format!("{} {}", editor, file_path);
            println!(
                " -- Your config file will be opened with the following command: `{}`",
                &command
            );
            println!(" -- After editing, the `setup` command will be run for you");
            let _ = prompt(" -- Press [ENTER] to open now or [CTRL+C] to exit and edit manually")?;
            open_file_in_fg(&editor, file_path)
                .map_err(|e| format_err!(ErrorKind::Config, "Error editing config file: {}", e))?;

            println!();
            let config = Config::from_settings_file(&config_path)?;
            let _setup = config.setup()?;
        }
        Ok(())
    }
}

/// Sqlite settings builder
#[derive(Debug, Clone, Default)]
pub struct SqliteSettingsBuilder {
    database_path: Option<String>,
    migration_location: Option<String>,
}
impl SqliteSettingsBuilder {
    /// Initialize an empty builder
    pub fn empty() -> Self {
        Self::default()
    }

    /// **Required** -- Set the absolute path of a database file.
    pub fn database_path<T: AsRef<Path>>(&mut self, p: T) -> Result<&mut Self> {
        let p = p.as_ref();
        let s = p
            .to_str()
            .ok_or_else(|| format_err!(ErrorKind::PathError, "Unicode path error: {:?}", p))?;
        self.database_path = Some(s.to_owned());
        Ok(self)
    }

    /// Set directory to look for migration files.
    ///
    /// This can be an absolute or relative path. An absolute path should be preferred.
    /// If a relative path is provided, the path will be assumed relative to either the
    /// settings file's directory if a settings file exists, or the current directory.
    pub fn migration_location<T: AsRef<Path>>(&mut self, p: T) -> Result<&mut Self> {
        let p = p.as_ref();
        let s = p
            .to_str()
            .ok_or_else(|| format_err!(ErrorKind::PathError, "Unicode path error: {:?}", p))?;
        self.migration_location = Some(s.to_owned());
        Ok(self)
    }

    /// Build a `Settings` object
    pub fn build(&self) -> Result<Settings> {
        let db_path = self
            .database_path
            .as_ref()
            .ok_or_else(|| format_err!(ErrorKind::Config, "Missing `database_path` parameter"))?
            .clone();
        {
            let p = Path::new(&db_path);
            if !p.is_absolute() {
                bail_fmt!(
                    ErrorKind::Config,
                    "Explicit settings database path must be absolute: {:?}",
                    p
                )
            }
        }
        let inner = ConfigurableSettings::Sqlite(SqliteSettings {
            database_type: "sqlite".into(),
            database_path: db_path,
            migration_location: self.migration_location.clone(),
        });
        Ok(Settings { inner })
    }
}

/// Postgres settings builder
#[derive(Debug, Clone, Default)]
pub struct PostgresSettingsBuilder {
    database_name: Option<String>,
    database_user: Option<String>,
    database_password: Option<String>,
    database_host: Option<String>,
    database_port: Option<String>,
    database_params: Option<BTreeMap<String, String>>,
    ssl_cert_file: Option<PathBuf>,
    migration_location: Option<String>,
}
impl PostgresSettingsBuilder {
    /// Initialize an empty builder
    pub fn empty() -> Self {
        Self::default()
    }

    /// **Required** -- Set the database name.
    pub fn database_name(&mut self, name: &str) -> &mut Self {
        self.database_name = Some(name.into());
        self
    }

    /// **Required** -- Set the database user.
    pub fn database_user(&mut self, user: &str) -> &mut Self {
        self.database_user = Some(user.into());
        self
    }

    /// **Required** -- Set the database password.
    pub fn database_password(&mut self, pass: &str) -> &mut Self {
        self.database_password = Some(pass.into());
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

    /// Set a collection of database connection parameters.
    pub fn database_params(&mut self, params: &[(&str, &str)]) -> &mut Self {
        let mut map = BTreeMap::new();
        for &(k, v) in params.iter() {
            map.insert(k.to_string(), v.to_string());
        }
        self.database_params = Some(map);
        self
    }

    /// Set a custom ssl cert file
    pub fn ssl_cert_file<P: AsRef<Path>>(&mut self, file: P) -> &mut Self {
        let file = file.as_ref().to_path_buf();
        self.ssl_cert_file = Some(file);
        self
    }

    /// Set directory to look for migration files.
    ///
    /// This can be an absolute or relative path. An absolute path should be preferred.
    /// If a relative path is provided, the path will be assumed relative to either the
    /// settings file's directory if a settings file exists, or the current directory.
    pub fn migration_location<T: AsRef<Path>>(&mut self, p: T) -> Result<&mut Self> {
        let p = p.as_ref();
        let s = p
            .to_str()
            .ok_or_else(|| format_err!(ErrorKind::PathError, "Unicode path error: {:?}", p))?;
        self.migration_location = Some(s.to_owned());
        Ok(self)
    }

    /// Build a `Settings` object
    pub fn build(&self) -> Result<Settings> {
        let inner = ConfigurableSettings::Postgres(PostgresSettings {
            database_type: "postgres".into(),
            database_name: self
                .database_name
                .as_ref()
                .ok_or_else(|| format_err!(ErrorKind::Config, "Missing `database_name` parameter"))?
                .clone(),
            database_user: self
                .database_user
                .as_ref()
                .ok_or_else(|| format_err!(ErrorKind::Config, "Missing `database_user` parameter"))?
                .clone(),
            database_password: self
                .database_password
                .as_ref()
                .ok_or_else(|| {
                    format_err!(ErrorKind::Config, "Missing `database_password` parameter")
                })?
                .clone(),
            database_host: self.database_host.clone(),
            database_port: self.database_port.clone(),
            database_params: self.database_params.clone(),
            ssl_cert_file: self.ssl_cert_file.clone(),
            migration_location: self.migration_location.clone(),
        });
        Ok(Settings { inner })
    }
}

/// MySQL settings builder
#[derive(Debug, Clone, Default)]
pub struct MySqlSettingsBuilder {
    database_name: Option<String>,
    database_user: Option<String>,
    database_password: Option<String>,
    database_host: Option<String>,
    database_port: Option<String>,
    database_params: Option<BTreeMap<String, String>>,
    migration_location: Option<String>,
}
impl MySqlSettingsBuilder {
    /// Initialize an empty builder
    pub fn empty() -> Self {
        Self::default()
    }

    /// **Required** -- Set the database name.
    pub fn database_name(&mut self, name: &str) -> &mut Self {
        self.database_name = Some(name.into());
        self
    }

    /// **Required** -- Set the database user.
    pub fn database_user(&mut self, user: &str) -> &mut Self {
        self.database_user = Some(user.into());
        self
    }

    /// **Required** -- Set the database password.
    pub fn database_password(&mut self, pass: &str) -> &mut Self {
        self.database_password = Some(pass.into());
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
    /// Set a collection of database connection parameters.
    pub fn database_params(&mut self, params: &[(&str, &str)]) -> &mut Self {
        let mut map = BTreeMap::new();
        for &(k, v) in params.iter() {
            map.insert(k.to_string(), v.to_string());
        }
        self.database_params = Some(map);
        self
    }

    /// Set directory to look for migration files.
    ///
    /// This can be an absolute or relative path. An absolute path should be preferred.
    /// If a relative path is provided, the path will be assumed relative to either the
    /// settings file's directory if a settings file exists, or the current directory.
    pub fn migration_location<T: AsRef<Path>>(&mut self, p: T) -> Result<&mut Self> {
        let p = p.as_ref();
        let s = p
            .to_str()
            .ok_or_else(|| format_err!(ErrorKind::PathError, "Unicode path error: {:?}", p))?;
        self.migration_location = Some(s.to_owned());
        Ok(self)
    }

    /// Build a `Settings` object
    pub fn build(&self) -> Result<Settings> {
        let inner = ConfigurableSettings::MySql(MySqlSettings {
            database_type: "mysql".into(),
            database_name: self
                .database_name
                .as_ref()
                .ok_or_else(|| format_err!(ErrorKind::Config, "Missing `database_name` parameter"))?
                .clone(),
            database_user: self
                .database_user
                .as_ref()
                .ok_or_else(|| format_err!(ErrorKind::Config, "Missing `database_user` parameter"))?
                .clone(),
            database_password: self
                .database_password
                .as_ref()
                .ok_or_else(|| {
                    format_err!(ErrorKind::Config, "Missing `database_password` parameter")
                })?
                .clone(),
            database_host: self.database_host.clone(),
            database_port: self.database_port.clone(),
            database_params: self.database_params.clone(),
            migration_location: self.migration_location.clone(),
        });
        Ok(Settings { inner })
    }
}

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct PostgresSettings {
    pub(crate) database_type: String,
    pub(crate) database_name: String,
    pub(crate) database_user: String,
    pub(crate) database_password: String,
    pub(crate) database_host: Option<String>,
    pub(crate) database_port: Option<String>,
    pub(crate) database_params: Option<BTreeMap<String, String>>,
    pub(crate) ssl_cert_file: Option<PathBuf>,
    pub(crate) migration_location: Option<String>,
}
impl PostgresSettings {
    pub(crate) fn connect_string(&self) -> Result<String> {
        let host = self
            .database_host
            .clone()
            .unwrap_or_else(|| "localhost".to_string());
        let host = if host.is_empty() {
            "localhost".to_string()
        } else {
            host
        };
        let host = encode(&host);

        let port = self
            .database_port
            .clone()
            .unwrap_or_else(|| "5432".to_string());
        let port = if port.is_empty() {
            "5432".to_string()
        } else {
            port
        };
        let port = encode(&port);

        let s = format!(
            "postgres://{user}:{pass}@{host}:{port}/{db_name}",
            user = encode(&self.database_user),
            pass = encode(&self.database_password),
            host = host,
            port = port,
            db_name = encode(&self.database_name)
        );

        let mut url = url::Url::parse(&s)?;

        if let Some(ref params) = self.database_params {
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
        Ok(url.to_string())
    }

    pub(crate) fn resolve_env_vars(&self) -> Self {
        let database_type = self.database_type.clone();

        let database_name = if self.database_name.starts_with("env:") {
            let var = self.database_name.trim_start_matches("env:");
            env::var(var).unwrap_or_else(|_| "".into())
        } else {
            self.database_name.to_string()
        };

        let database_user = if self.database_user.starts_with("env:") {
            let var = self.database_user.trim_start_matches("env:");
            env::var(var).unwrap_or_else(|_| "".into())
        } else {
            self.database_user.to_string()
        };

        let database_password = if self.database_password.starts_with("env:") {
            let var = self.database_password.trim_start_matches("env:");
            env::var(var).unwrap_or_else(|_| "".into())
        } else {
            self.database_password.to_string()
        };

        let database_host = self.database_host.as_ref().map(|maybe_str| {
            if maybe_str.starts_with("env:") {
                let var = maybe_str.trim_start_matches("env:");
                env::var(var).unwrap_or_else(|_| "".into())
            } else {
                maybe_str.to_string()
            }
        });

        let database_port = self.database_port.as_ref().map(|maybe_str| {
            if maybe_str.starts_with("env:") {
                let var = maybe_str.trim_start_matches("env:");
                env::var(var).unwrap_or_else(|_| "".into())
            } else {
                maybe_str.to_string()
            }
        });

        let database_params = self.database_params.as_ref().map(|vars| {
            vars.iter().fold(BTreeMap::new(), |mut acc, (k, v)| {
                let val = if v.starts_with("env:") {
                    let v = v.trim_start_matches("env:");
                    env::var(v).unwrap_or_else(|_| "".into())
                } else {
                    v.clone()
                };
                acc.insert(k.clone(), val);
                acc
            })
        });

        let ssl_cert_file = self.ssl_cert_file.clone();

        let migration_location = self.migration_location.as_ref().map(|maybe_str| {
            if maybe_str.starts_with("env:") {
                let var = maybe_str.trim_start_matches("env:");
                env::var(var).unwrap_or_else(|_| "".into())
            } else {
                maybe_str.to_string()
            }
        });

        Self {
            database_type,
            database_name,
            database_user,
            database_password,
            database_host,
            database_port,
            database_params,
            ssl_cert_file,
            migration_location,
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct MySqlSettings {
    pub(crate) database_type: String,
    pub(crate) database_name: String,
    pub(crate) database_user: String,
    pub(crate) database_password: String,
    pub(crate) database_host: Option<String>,
    pub(crate) database_port: Option<String>,
    pub(crate) database_params: Option<BTreeMap<String, String>>,
    pub(crate) migration_location: Option<String>,
}
impl MySqlSettings {
    pub(crate) fn connect_string(&self) -> Result<String> {
        let host = self
            .database_host
            .clone()
            .unwrap_or_else(|| "localhost".to_string());
        let host = if host.is_empty() {
            "localhost".to_string()
        } else {
            host
        };
        let host = encode(&host);

        let port = self
            .database_port
            .clone()
            .unwrap_or_else(|| "3306".to_string());
        let port = if port.is_empty() {
            "3306".to_string()
        } else {
            port
        };
        let port = encode(&port);

        let s = format!(
            "mysql://{user}:{pass}@{host}:{port}/{db_name}",
            user = encode(&self.database_user),
            pass = encode(&self.database_password),
            host = host,
            port = port,
            db_name = encode(&self.database_name)
        );

        let mut url = url::Url::parse(&s)?;

        if let Some(ref params) = self.database_params {
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
        Ok(url.to_string())
    }

    pub(crate) fn resolve_env_vars(&self) -> Self {
        let database_type = self.database_type.clone();

        let database_name = if self.database_name.starts_with("env:") {
            let var = self.database_name.trim_start_matches("env:");
            env::var(var).unwrap_or_else(|_| "".into())
        } else {
            self.database_name.to_string()
        };

        let database_user = if self.database_user.starts_with("env:") {
            let var = self.database_user.trim_start_matches("env:");
            env::var(var).unwrap_or_else(|_| "".into())
        } else {
            self.database_user.to_string()
        };

        let database_password = if self.database_password.starts_with("env:") {
            let var = self.database_password.trim_start_matches("env:");
            env::var(var).unwrap_or_else(|_| "".into())
        } else {
            self.database_password.to_string()
        };

        let database_host = self.database_host.as_ref().map(|maybe_str| {
            if maybe_str.starts_with("env:") {
                let var = maybe_str.trim_start_matches("env:");
                env::var(var).unwrap_or_else(|_| "".into())
            } else {
                maybe_str.to_string()
            }
        });

        let database_port = self.database_port.as_ref().map(|maybe_str| {
            if maybe_str.starts_with("env:") {
                let var = maybe_str.trim_start_matches("env:");
                env::var(var).unwrap_or_else(|_| "".into())
            } else {
                maybe_str.to_string()
            }
        });

        let database_params = self.database_params.as_ref().map(|vars| {
            vars.iter().fold(BTreeMap::new(), |mut acc, (k, v)| {
                let val = if v.starts_with("env:") {
                    let v = v.trim_start_matches("env:");
                    env::var(v).unwrap_or_else(|_| "".into())
                } else {
                    v.clone()
                };
                acc.insert(k.clone(), val);
                acc
            })
        });

        let migration_location = self.migration_location.as_ref().map(|maybe_str| {
            if maybe_str.starts_with("env:") {
                let var = maybe_str.trim_start_matches("env:");
                env::var(var).unwrap_or_else(|_| "".into())
            } else {
                maybe_str.to_string()
            }
        });

        Self {
            database_type,
            database_name,
            database_user,
            database_password,
            database_host,
            database_port,
            database_params,
            migration_location,
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct SqliteSettings {
    pub(crate) database_type: String,
    pub(crate) database_path: String,
    pub(crate) migration_location: Option<String>,
}
impl SqliteSettings {
    pub(crate) fn resolve_env_vars(&self) -> Self {
        let database_type = self.database_type.clone();

        let database_path = if self.database_path.starts_with("env:") {
            let var = self.database_path.trim_start_matches("env:");
            env::var(var).unwrap_or_else(|_| "".into())
        } else {
            self.database_path.to_string()
        };

        let migration_location = self.migration_location.as_ref().map(|maybe_str| {
            if maybe_str.starts_with("env:") {
                let var = maybe_str.trim_start_matches("env:");
                env::var(var).unwrap_or_else(|_| "".into())
            } else {
                maybe_str.to_string()
            }
        });
        Self {
            database_type,
            database_path,
            migration_location,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum ConfigurableSettings {
    Postgres(PostgresSettings),
    Sqlite(SqliteSettings),
    MySql(MySqlSettings),
}
impl ConfigurableSettings {
    pub(crate) fn db_kind(&self) -> DbKind {
        match *self {
            ConfigurableSettings::Sqlite(_) => DbKind::Sqlite,
            ConfigurableSettings::Postgres(_) => DbKind::Postgres,
            ConfigurableSettings::MySql(_) => DbKind::MySql,
        }
    }

    pub(crate) fn migration_location(&self) -> Option<PathBuf> {
        match *self {
            ConfigurableSettings::Sqlite(ref s) => s.migration_location.as_ref().map(PathBuf::from),
            ConfigurableSettings::Postgres(ref s) => {
                s.migration_location.as_ref().map(PathBuf::from)
            }
            ConfigurableSettings::MySql(ref s) => s.migration_location.as_ref().map(PathBuf::from),
        }
    }

    pub(crate) fn database_path(&self) -> Result<PathBuf> {
        match *self {
            ConfigurableSettings::Sqlite(ref s) => Ok(PathBuf::from(&s.database_path)),
            ConfigurableSettings::Postgres(ref s) => bail_fmt!(
                ErrorKind::Config,
                "Cannot generate database_path for database-type: {}",
                s.database_type
            ),
            ConfigurableSettings::MySql(ref s) => bail_fmt!(
                ErrorKind::Config,
                "Cannot generate database_path for database-type: {}",
                s.database_type
            ),
        }
    }

    pub(crate) fn connect_string(&self) -> Result<String> {
        match *self {
            ConfigurableSettings::Postgres(ref s) => s.connect_string(),
            ConfigurableSettings::MySql(ref s) => s.connect_string(),
            ConfigurableSettings::Sqlite(ref s) => bail_fmt!(
                ErrorKind::Config,
                "Cannot generate connect-string for database-type: {}",
                s.database_type
            ),
        }
    }

    pub(crate) fn ssl_cert_file(&self) -> Option<PathBuf> {
        match *self {
            ConfigurableSettings::Postgres(ref s) => s.ssl_cert_file.clone(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
/// Project settings
///
/// These settings are serialized and saved in a project `Migrant.toml` config file
/// or defined explicitly in source using the provided builder methods.
pub struct Settings {
    pub(crate) inner: ConfigurableSettings,
}
impl Settings {
    /// Initialize from a serialized settings file
    pub fn from_file<T: AsRef<Path>>(path: T) -> Result<Self> {
        #[derive(Deserialize)]
        struct DbTypeField {
            database_type: String,
        }
        let mut f = fs::File::open(path.as_ref())?;
        let mut content = String::new();
        f.read_to_string(&mut content)?;

        let type_field = toml::from_str::<DbTypeField>(&content)?;
        let inner = match type_field.database_type.as_ref() {
            "sqlite" => {
                let settings = toml::from_str::<SqliteSettings>(&content)?;
                let settings = settings.resolve_env_vars();
                ConfigurableSettings::Sqlite(settings)
            }
            "postgres" => {
                let settings = toml::from_str::<PostgresSettings>(&content)?;
                let settings = settings.resolve_env_vars();
                ConfigurableSettings::Postgres(settings)
            }
            "mysql" => {
                let settings = toml::from_str::<MySqlSettings>(&content)?;
                let settings = settings.resolve_env_vars();
                ConfigurableSettings::MySql(settings)
            }
            t => bail_fmt!(ErrorKind::Config, "Invalid database_type: {:?}", t),
        };
        Ok(Self { inner })
    }

    /// Initialize a `SqliteSettingsBuilder` to be configured
    pub fn configure_sqlite() -> SqliteSettingsBuilder {
        SqliteSettingsBuilder::default()
    }

    /// Initialize a `PostgresSettingsBuilder` to be configured
    pub fn configure_postgres() -> PostgresSettingsBuilder {
        PostgresSettingsBuilder::default()
    }

    /// Initialize a `MySqlSettingsBuilder` to be configured
    pub fn configure_mysql() -> MySqlSettingsBuilder {
        MySqlSettingsBuilder::default()
    }
}

#[derive(Debug, Clone)]
/// Full project configuration
pub struct Config {
    pub(crate) settings: Settings,
    pub(crate) settings_path: Option<PathBuf>,
    pub(crate) applied: Vec<String>,
    pub(crate) migrations: Option<Vec<Box<dyn Migratable>>>,
    pub(crate) cli_compatible: bool,
}
impl Config {
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
    /// extern crate migrant_lib;
    /// use migrant_lib::{
    ///     Config, search_for_settings_file,
    ///     EmbeddedMigration, FileMigration, FnMigration
    /// };
    ///
    /// # fn run() -> Result<(), Box<dyn std::error::Error>> {
    /// mod migrations {
    ///     use super::*;
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
    /// # #[cfg(any(feature="d-sqlite", feature="d-postgres", feature="d-mysql"))]
    /// config.use_migrations(&[
    ///     EmbeddedMigration::with_tag("create-users-table")
    ///         .up(include_str!("../migrations/embedded/create_users_table/up.sql"))
    ///         .down(include_str!("../migrations/embedded/create_users_table/down.sql"))
    ///         .boxed(),
    ///     FileMigration::with_tag("create-places-table")
    ///         .up("migrations/embedded/create_places_table/up.sql")?
    ///         .down("migrations/embedded/create_places_table/down.sql")?
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
        let mut set = HashSet::with_capacity(migrations.len());
        let mut migs = Vec::with_capacity(migrations.len());
        for mig in migrations {
            let tag = mig.tag();
            if self.cli_compatible {
                if invalid_full_tag(&tag) {
                    bail_fmt!(
                        ErrorKind::TagError,
                        "When `cli_compatible=true` tags must be timestamped, \
                         following: `[0-9]{{14}}_[a-z0-9-]+`. Found tag: `{}`",
                        tag
                    )
                }
            } else if invalid_optional_stamp_tag(&tag) {
                bail_fmt!(
                    ErrorKind::TagError,
                    "When `cli_compatible=false` (default) tags may only contain, \
                     `[a-z0-9-]` and may be optionally prefixed with a timestamp \
                     following: `([0-9]{{14}}_)?[a-z0-9-]+`. Found tag: `{}`",
                    tag
                )
            }
            if set.contains(&tag) {
                bail_fmt!(
                    ErrorKind::TagError,
                    "Tags must be unique. Found duplicate: {}",
                    tag
                )
            }
            set.insert(tag);
            migs.push(mig.clone());
        }
        self.migrations = Some(migs);
        Ok(self)
    }

    /// Migrations are explicitly defined
    pub fn is_explicit(&self) -> bool {
        self.migrations.is_some()
    }

    /// Toggle cli compatible tag validation.
    ///
    /// **Note:** Make sure any calls to `Config::use_cli_compatible_tags` happen
    /// **before** any calls to `Config::reload` or `Config::use_migrations` since
    /// this is dependent on the tag format being used.
    ///
    /// Defaults to `false`. When `cli_compatible` is set to `true`, migration
    /// tags will be validated in a manner compatible with the migrant CLI tool.
    /// Tags must be prefixed with a timestamp, following: `[0-9]{14}_[a-z0-9-]+`.
    /// When not enabled (the default), tag timestamps are optional and
    /// the migrant CLI tool will not be able to identify tags.
    pub fn use_cli_compatible_tags(&mut self, compat: bool) {
        self.cli_compatible = compat;
    }

    /// Check the current cli compatibility
    pub fn is_cli_compatible(&self) -> bool {
        self.cli_compatible
    }

    /// Check that migration tags conform to naming requirements.
    /// If CLI compatibility is enabled, then tags must be prefixed with a timestamp
    /// following: `[0-9]{14}_[a-z0-9-]+` which is the format generated by the migrant
    /// CLI tool and `migrant_lib::new`. When CLI compatibility is disabled (default).
    /// tags may only contain `[a-z0-9-]`, but can still be optionally prefixed with
    /// a timestamp following: `([0-9]{14}_)?[a-z0-9-]+`.
    fn check_saved_tag(&self, tag: &str) -> Result<()> {
        if self.cli_compatible {
            if invalid_full_tag(tag) {
                bail_fmt!(
                    ErrorKind::Migration,
                    "Found a non-conforming tag in the database: `{}`. \
                     Generated/CLI-compatible tags must follow `[0-9]{{14}}_[a-z0-9-]+`",
                    tag
                )
            }
        } else if invalid_optional_stamp_tag(tag) {
            bail_fmt!(
                ErrorKind::Migration,
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
            Some(path) => Config::from_settings_file(path)?,
            None => self.clone(),
        };
        config.cli_compatible = self.cli_compatible;
        config.migrations = self.migrations.clone();
        let applied = config.load_applied()?;
        config.applied = applied;
        Ok(config)
    }

    /// Initialize a `Config` from a settings file at the given path.
    /// This does not query the database for applied migrations.
    pub fn from_settings_file<T: AsRef<Path>>(path: T) -> Result<Config> {
        let path = path.as_ref();
        let settings = Settings::from_file(path)?;
        Ok(Config {
            settings_path: Some(path.to_owned()),
            settings,
            applied: vec![],
            migrations: None,
            cli_compatible: false,
        })
    }

    /// Initialize a `Config` using an explicitly created `Settings` object.
    /// This alleviates the need for a settings file.
    /// This does not query the database for applied migrations.
    ///
    /// ```rust,no_run
    /// # extern crate migrant_lib;
    /// # use migrant_lib::{Settings, Config};
    /// # fn main() { run().unwrap(); }
    /// # fn run() -> Result<(), Box<dyn std::error::Error>> {
    /// let settings = Settings::configure_sqlite()
    ///     .database_path("/absolute/path/to/db.db")?
    ///     .migration_location("/absolute/path/to/migration_dir")?
    ///     .build()?;
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
            cli_compatible: false,
        }
    }

    /// Load the applied migrations from the database migration table
    pub(crate) fn load_applied(&self) -> Result<Vec<String>> {
        if !self.migration_table_exists()? {
            bail_fmt!(
                ErrorKind::Migration,
                "`__migrant_migrations` table is missing, maybe try re-setting-up? -> `setup`"
            )
        }

        let applied = match self.settings.inner.db_kind() {
            DbKind::Sqlite => drivers::sqlite::select_migrations(&self.database_path_string()?)?,
            DbKind::Postgres => drivers::pg::select_migrations(
                self.ssl_cert_file().as_deref(),
                &self.connect_string()?,
            )?,
            DbKind::MySql => drivers::mysql::select_migrations(&self.connect_string()?)?,
        };
        let mut tags = vec![];
        for tag in applied.into_iter() {
            self.check_saved_tag(&tag)?;
            tags.push(tag);
        }
        let tags = if !self.cli_compatible {
            tags
        } else {
            let mut stamped = tags
                .into_iter()
                .map(|tag| {
                    let stamp = tag.split('_').next().ok_or_else(|| {
                        format_err!(ErrorKind::TagError, "Invalid tag format: {:?}", tag)
                    })?;
                    let stamp = chrono::Utc.datetime_from_str(stamp, DT_FORMAT)?;
                    Ok((stamp, tag.clone()))
                })
                .collect::<Result<Vec<_>>>()?;
            stamped.sort_by(|a, b| a.0.cmp(&b.0));
            stamped.into_iter().map(|tup| tup.1).collect::<Vec<_>>()
        };
        Ok(tags)
    }

    /// Check if a __migrant_migrations table exists
    pub(crate) fn migration_table_exists(&self) -> Result<bool> {
        match self.settings.inner.db_kind() {
            DbKind::Sqlite => {
                drivers::sqlite::migration_table_exists(&self.database_path_string()?)
            }
            DbKind::Postgres => drivers::pg::migration_table_exists(
                self.ssl_cert_file().as_deref(),
                &self.connect_string()?,
            ),
            DbKind::MySql => drivers::mysql::migration_table_exists(&self.connect_string()?),
        }
    }

    /// Insert given tag into database migration table
    pub(crate) fn insert_migration_tag(&self, tag: &str) -> Result<()> {
        match self.settings.inner.db_kind() {
            DbKind::Sqlite => {
                drivers::sqlite::insert_migration_tag(&self.database_path_string()?, tag)?
            }
            DbKind::Postgres => drivers::pg::insert_migration_tag(
                self.ssl_cert_file().as_deref(),
                &self.connect_string()?,
                tag,
            )?,
            DbKind::MySql => drivers::mysql::insert_migration_tag(&self.connect_string()?, tag)?,
        };
        Ok(())
    }

    /// Remove a given tag from the database migration table
    pub(crate) fn delete_migration_tag(&self, tag: &str) -> Result<()> {
        match self.settings.inner.db_kind() {
            DbKind::Sqlite => {
                drivers::sqlite::remove_migration_tag(&self.database_path_string()?, tag)?
            }
            DbKind::Postgres => drivers::pg::remove_migration_tag(
                self.ssl_cert_file().as_deref(),
                &self.connect_string()?,
                tag,
            )?,
            DbKind::MySql => drivers::mysql::remove_migration_tag(&self.connect_string()?, tag)?,
        };
        Ok(())
    }

    /// Initialize a new settings file in the given directory
    pub fn init_in<T: AsRef<Path>>(dir: T) -> SettingsFileInitializer {
        SettingsFileInitializer::new(dir.as_ref())
    }

    /// Confirm the database can be accessed and setup the database
    /// migrations table if it doesn't already exist
    pub fn setup(&self) -> Result<bool> {
        debug!(" ** Confirming database credentials...");
        match self.settings.inner {
            ConfigurableSettings::Sqlite(_) => {
                let created = drivers::sqlite::create_file_if_missing(&self.database_path()?)?;
                debug!("    - checking if db file already exists...");
                if created {
                    debug!("    - db not found... creating now... ✓")
                } else {
                    debug!("    - db already exists ✓");
                }
            }
            ConfigurableSettings::Postgres(ref s) => {
                let conn_str = s.connect_string()?;
                let can_connect = drivers::pg::can_connect(s.ssl_cert_file.as_deref(), &conn_str)?;
                if !can_connect {
                    error!(" ERROR: Unable to connect to {}", conn_str);
                    error!("        Please initialize your database and user and then run `setup`");
                    error!("\n  ex) sudo -u postgres createdb {}", s.database_name);
                    error!("      sudo -u postgres createuser {}", s.database_user);
                    error!(
                        "      sudo -u postgres psql -c \"alter user {} with password '****'\"",
                        s.database_user
                    );
                    error!("");
                    bail_fmt!(
                        ErrorKind::Config,
                        "Cannot connect to postgres database with connection string: {:?}. \
                         Do the database & user exist?",
                        conn_str
                    );
                } else {
                    debug!("    - Connection confirmed ✓");
                }
            }
            ConfigurableSettings::MySql(ref s) => {
                let conn_str = s.connect_string()?;
                let can_connect = drivers::mysql::can_connect(&conn_str)?;
                if !can_connect {
                    let localhost = String::from("localhost");
                    error!(" ERROR: Unable to connect to {}", conn_str);
                    error!("        Please initialize your database and user and then run `setup`");
                    error!(
                        "\n  ex) mysql -u root -p -e \"create database {};\"",
                        s.database_name
                    );
                    error!("      mysql -u root -p -e \"create user '{}'@'{}' identified by '*****';\"",
                           s.database_user, s.database_host.as_ref().unwrap_or(&localhost));
                    error!(
                        "      mysql -u root -p e \"grant all privileges on {}.* to '{}'@'{}';\"",
                        s.database_name,
                        s.database_user,
                        s.database_host.as_ref().unwrap_or(&localhost)
                    );
                    error!("      mysql -u root -p e \"flush privileges;\"");
                    error!("");
                    bail_fmt!(
                        ErrorKind::Config,
                        "Cannot connect to mysql database with connection string: {:?}. \
                         Do the database & user exist?",
                        conn_str
                    );
                } else {
                    debug!("    - Connection confirmed ✓");
                }
            }
        }

        debug!("\n ** Setting up migrations table");
        let table_created = match self.settings.inner {
            ConfigurableSettings::Sqlite(_) => {
                drivers::sqlite::migration_setup(&self.database_path()?)?
            }
            ConfigurableSettings::Postgres(ref s) => {
                let conn_str = s.connect_string()?;
                drivers::pg::migration_setup(self.ssl_cert_file().as_deref(), &conn_str)?
            }
            ConfigurableSettings::MySql(ref s) => {
                let conn_str = s.connect_string()?;
                drivers::mysql::migration_setup(&conn_str)?
            }
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
    ///
    /// The location returned is dependent on whether an absolute or relative path
    /// was provided to `migration_location` in either a settings file or settings builder.
    /// If an absolute path was provided, that same path is returned.
    /// If a relative path was provided, the path returned will be relative
    /// to either the settings file's directory if a settings file exists, or
    /// the current directory.
    #[deprecated(since = "0.18.1", note = "renamed to `migration_location`")]
    pub fn migration_dir(&self) -> Result<PathBuf> {
        let path = self
            .settings
            .inner
            .migration_location()
            .unwrap_or_else(|| PathBuf::from("migrations"));
        Ok(if path.is_absolute() {
            path
        } else {
            let cur_dir = env::current_dir()?;
            let base_path = match self.settings_path.as_ref() {
                Some(s_path) => s_path.parent().ok_or_else(|| {
                    format_err!(
                        ErrorKind::PathError,
                        "Unable to determine parent path: {:?}",
                        s_path
                    )
                })?,
                None => &cur_dir,
            };
            base_path.join(path)
        })
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
        Ok(if path.is_absolute() {
            path
        } else {
            let cur_dir = env::current_dir()?;
            let base_path = match self.settings_path.as_ref() {
                Some(s_path) => s_path.parent().ok_or_else(|| {
                    format_err!(
                        ErrorKind::PathError,
                        "Unable to determine parent path: {:?}",
                        s_path
                    )
                })?,
                None => &cur_dir,
            };
            base_path.join(path)
        })
    }

    /// Return the database type
    pub fn database_type(&self) -> DbKind {
        self.settings.inner.db_kind()
    }

    fn database_path_string(&self) -> Result<String> {
        let path = self.database_path()?;
        let path = path
            .to_str()
            .ok_or_else(|| format_err!(ErrorKind::PathError, "Invalid utf8 path: {:?}", path))?
            .to_owned();
        Ok(path)
    }

    /// Return the absolute path to the database file. This is intended for
    /// sqlite databases only
    pub fn database_path(&self) -> Result<PathBuf> {
        let path = self.settings.inner.database_path()?;
        if path.is_absolute() {
            Ok(path)
        } else {
            let spath =
                Path::new(self.settings_path.as_ref().ok_or_else(|| {
                    format_err!(ErrorKind::Config, "Settings path not specified")
                })?);
            let spath = spath.parent().ok_or_else(|| {
                format_err!(
                    ErrorKind::PathError,
                    "Unable to determine parent path: {:?}",
                    spath
                )
            })?;
            Ok(spath.join(&path))
        }
    }

    /// Generate a database connection string.
    /// Not intended for file-based databases (sqlite)
    pub fn connect_string(&self) -> Result<String> {
        self.settings.inner.connect_string()
    }

    pub fn ssl_cert_file(&self) -> Option<PathBuf> {
        self.settings.inner.ssl_cert_file()
    }
}
