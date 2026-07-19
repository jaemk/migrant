/*!
Settings builders
*/
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::errors::*;
use crate::macros::{bail, err};
use crate::SQLITE_MEMORY_PATH;

use super::settings::{DbSettings, ServerSettings, Settings, SqliteSettings};

fn path_to_string(p: &Path) -> Result<String> {
    p.to_str()
        .map(str::to_owned)
        .ok_or_else(|| err!(PathError, "Unicode path error: {:?}", p))
}

/// Sqlite settings builder
#[derive(Debug, Clone, Default)]
pub struct SqliteSettingsBuilder {
    pub(crate) database_path: Option<String>,
    pub(crate) migration_location: Option<String>,
}

impl SqliteSettingsBuilder {
    /// Initialize an empty builder
    pub fn empty() -> Self {
        Self::default()
    }

    /// **Required** (unless `memory` is used) -- Set the path of a database file.
    ///
    /// The special path `:memory:` selects an in-memory database.
    ///
    /// When this builder is used directly with `build()` (or passed to
    /// `Config::with_settings`), the path must be absolute. When it is instead passed to
    /// `Config::init_in(...).with_sqlite_options(...)`, a relative path is also accepted --
    /// it is written into the generated settings file and resolved relative to that
    /// settings file's directory when the config is later loaded.
    pub fn database_path<T: AsRef<Path>>(mut self, p: T) -> Result<Self> {
        self.database_path = Some(path_to_string(p.as_ref())?);
        Ok(self)
    }

    /// Use an in-memory database.
    ///
    /// The database connection is established once and then kept alive
    /// (shared by all clones of the built `Config`) so migrations and
    /// application queries all see the same database.
    pub fn memory(mut self) -> Self {
        self.database_path = Some(SQLITE_MEMORY_PATH.to_string());
        self
    }

    /// Set directory to look for migration files.
    ///
    /// This can be an absolute or relative path. An absolute path should be preferred.
    /// If a relative path is provided, the path will be assumed relative to either the
    /// settings file's directory if a settings file exists, or the current directory.
    pub fn migration_location<T: AsRef<Path>>(mut self, p: T) -> Result<Self> {
        self.migration_location = Some(path_to_string(p.as_ref())?);
        Ok(self)
    }

    /// Build a `Settings` object
    pub fn build(&self) -> Result<Settings> {
        let database_path = self
            .database_path
            .clone()
            .ok_or_else(|| err!(Config, "Missing `database_path` parameter"))?;
        if database_path != SQLITE_MEMORY_PATH && !Path::new(&database_path).is_absolute() {
            bail!(
                Config,
                "Explicit settings database path must be absolute: {:?}",
                database_path
            )
        }
        Ok(Settings::new(DbSettings::Sqlite(SqliteSettings {
            database_path,
            migration_location: self.migration_location.clone(),
        })))
    }
}

/// Fields shared by the server-database (postgres, mysql) builders
#[derive(Debug, Clone, Default)]
pub(crate) struct ServerSettingsBuilder {
    pub(crate) database_name: Option<String>,
    pub(crate) database_user: Option<String>,
    pub(crate) database_password: Option<String>,
    pub(crate) database_host: Option<String>,
    pub(crate) database_port: Option<String>,
    pub(crate) database_params: Option<BTreeMap<String, String>>,
    pub(crate) ssl_cert_file: Option<PathBuf>,
    pub(crate) migration_location: Option<String>,
}

impl ServerSettingsBuilder {
    fn build(&self) -> Result<ServerSettings> {
        Ok(ServerSettings {
            database_name: self
                .database_name
                .clone()
                .ok_or_else(|| err!(Config, "Missing `database_name` parameter"))?,
            database_user: self
                .database_user
                .clone()
                .ok_or_else(|| err!(Config, "Missing `database_user` parameter"))?,
            database_password: self
                .database_password
                .clone()
                .ok_or_else(|| err!(Config, "Missing `database_password` parameter"))?,
            database_host: self.database_host.clone(),
            database_port: self.database_port.clone(),
            database_params: self.database_params.clone(),
            ssl_cert_file: self.ssl_cert_file.clone(),
            migration_location: self.migration_location.clone(),
        })
    }

    fn set_params(&mut self, params: &[(&str, &str)]) {
        self.database_params = Some(
            params
                .iter()
                .map(|&(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        );
    }
}

macro_rules! server_builder_methods {
    () => {
        /// **Required** -- Set the database name.
        pub fn database_name(mut self, name: &str) -> Self {
            self.inner.database_name = Some(name.into());
            self
        }

        /// **Required** -- Set the database user.
        pub fn database_user(mut self, user: &str) -> Self {
            self.inner.database_user = Some(user.into());
            self
        }

        /// **Required** -- Set the database password.
        pub fn database_password(mut self, pass: &str) -> Self {
            self.inner.database_password = Some(pass.into());
            self
        }

        /// Set the database host.
        pub fn database_host(mut self, host: &str) -> Self {
            self.inner.database_host = Some(host.into());
            self
        }

        /// Set the database port.
        pub fn database_port(mut self, port: u16) -> Self {
            self.inner.database_port = Some(port.to_string());
            self
        }

        /// Set a collection of database connection parameters.
        pub fn database_params(mut self, params: &[(&str, &str)]) -> Self {
            self.inner.set_params(params);
            self
        }

        /// Set directory to look for migration files.
        ///
        /// This can be an absolute or relative path. An absolute path should be preferred.
        /// If a relative path is provided, the path will be assumed relative to either the
        /// settings file's directory if a settings file exists, or the current directory.
        pub fn migration_location<T: AsRef<Path>>(mut self, p: T) -> Result<Self> {
            self.inner.migration_location = Some(path_to_string(p.as_ref())?);
            Ok(self)
        }
    };
}

/// Postgres settings builder
#[derive(Debug, Clone, Default)]
pub struct PostgresSettingsBuilder {
    pub(crate) inner: ServerSettingsBuilder,
}

impl PostgresSettingsBuilder {
    /// Initialize an empty builder
    pub fn empty() -> Self {
        Self::default()
    }

    server_builder_methods!();

    /// Set a custom ssl cert file
    pub fn ssl_cert_file<P: AsRef<Path>>(mut self, file: P) -> Self {
        self.inner.ssl_cert_file = Some(file.as_ref().to_path_buf());
        self
    }

    /// Build a `Settings` object
    pub fn build(&self) -> Result<Settings> {
        Ok(Settings::new(DbSettings::Postgres(self.inner.build()?)))
    }
}

/// MySQL settings builder
#[derive(Debug, Clone, Default)]
pub struct MySqlSettingsBuilder {
    pub(crate) inner: ServerSettingsBuilder,
}

impl MySqlSettingsBuilder {
    /// Initialize an empty builder
    pub fn empty() -> Self {
        Self::default()
    }

    server_builder_methods!();

    /// Build a `Settings` object
    pub fn build(&self) -> Result<Settings> {
        Ok(Settings::new(DbSettings::MySql(self.inner.build()?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The owned (`self -> Self` / `self -> Result<Self>`) setters must chain
    // without an intermediate `mut` binding and carry every value through.

    #[test]
    fn sqlite_owned_setters_chain_and_build() {
        let settings = SqliteSettingsBuilder::empty()
            .database_path("/abs/path/to/my.db")
            .unwrap()
            .migration_location("/abs/migrations")
            .unwrap()
            .build()
            .unwrap();
        match settings.inner {
            DbSettings::Sqlite(s) => {
                assert_eq!(s.database_path, "/abs/path/to/my.db");
                assert_eq!(s.migration_location.as_deref(), Some("/abs/migrations"));
            }
            other => panic!("expected sqlite settings, got {:?}", other),
        }
    }

    #[test]
    fn sqlite_memory_owned_setter_chains() {
        // `memory()` consumes and returns owned self, chainable into `build()`.
        let builder = SqliteSettingsBuilder::empty().memory();
        assert_eq!(builder.database_path.as_deref(), Some(SQLITE_MEMORY_PATH));
        let settings = builder.build().unwrap();
        match settings.inner {
            DbSettings::Sqlite(s) => assert_eq!(s.database_path, SQLITE_MEMORY_PATH),
            other => panic!("expected sqlite settings, got {:?}", other),
        }
    }

    #[test]
    fn postgres_owned_server_setters_chain_and_build() {
        let settings = PostgresSettingsBuilder::empty()
            .database_name("mydb")
            .database_user("me")
            .database_password("secret")
            .database_host("db.example.com")
            .database_port(4444)
            .database_params(&[("sslmode", "require")])
            .ssl_cert_file("/certs/db.pem")
            .migration_location("/abs/migrations")
            .unwrap()
            .build()
            .unwrap();
        match settings.inner {
            DbSettings::Postgres(s) => {
                assert_eq!(s.database_name, "mydb");
                assert_eq!(s.database_user, "me");
                assert_eq!(s.database_password, "secret");
                assert_eq!(s.database_host.as_deref(), Some("db.example.com"));
                assert_eq!(s.database_port.as_deref(), Some("4444"));
                assert_eq!(
                    s.database_params.unwrap().get("sslmode"),
                    Some(&"require".to_string())
                );
                assert_eq!(s.ssl_cert_file, Some(PathBuf::from("/certs/db.pem")));
                assert_eq!(s.migration_location.as_deref(), Some("/abs/migrations"));
            }
            other => panic!("expected postgres settings, got {:?}", other),
        }
    }

    #[test]
    fn mysql_owned_server_setters_chain_and_build() {
        let settings = MySqlSettingsBuilder::empty()
            .database_name("mydb")
            .database_user("me")
            .database_password("secret")
            .database_port(3307)
            .build()
            .unwrap();
        match settings.inner {
            DbSettings::MySql(s) => {
                assert_eq!(s.database_name, "mydb");
                assert_eq!(s.database_port.as_deref(), Some("3307"));
            }
            other => panic!("expected mysql settings, got {:?}", other),
        }
    }
}
