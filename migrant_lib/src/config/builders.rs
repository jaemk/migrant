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
    pub fn database_path<T: AsRef<Path>>(&mut self, p: T) -> Result<&mut Self> {
        self.database_path = Some(path_to_string(p.as_ref())?);
        Ok(self)
    }

    /// Use an in-memory database.
    ///
    /// The database connection is established once and then kept alive
    /// (shared by all clones of the built `Config`) so migrations and
    /// application queries all see the same database.
    pub fn memory(&mut self) -> &mut Self {
        self.database_path = Some(SQLITE_MEMORY_PATH.to_string());
        self
    }

    /// Set directory to look for migration files.
    ///
    /// This can be an absolute or relative path. An absolute path should be preferred.
    /// If a relative path is provided, the path will be assumed relative to either the
    /// settings file's directory if a settings file exists, or the current directory.
    pub fn migration_location<T: AsRef<Path>>(&mut self, p: T) -> Result<&mut Self> {
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
        pub fn database_name(&mut self, name: &str) -> &mut Self {
            self.inner.database_name = Some(name.into());
            self
        }

        /// **Required** -- Set the database user.
        pub fn database_user(&mut self, user: &str) -> &mut Self {
            self.inner.database_user = Some(user.into());
            self
        }

        /// **Required** -- Set the database password.
        pub fn database_password(&mut self, pass: &str) -> &mut Self {
            self.inner.database_password = Some(pass.into());
            self
        }

        /// Set the database host.
        pub fn database_host(&mut self, host: &str) -> &mut Self {
            self.inner.database_host = Some(host.into());
            self
        }

        /// Set the database port.
        pub fn database_port(&mut self, port: u16) -> &mut Self {
            self.inner.database_port = Some(port.to_string());
            self
        }

        /// Set a collection of database connection parameters.
        pub fn database_params(&mut self, params: &[(&str, &str)]) -> &mut Self {
            self.inner.set_params(params);
            self
        }

        /// Set directory to look for migration files.
        ///
        /// This can be an absolute or relative path. An absolute path should be preferred.
        /// If a relative path is provided, the path will be assumed relative to either the
        /// settings file's directory if a settings file exists, or the current directory.
        pub fn migration_location<T: AsRef<Path>>(&mut self, p: T) -> Result<&mut Self> {
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
    pub fn ssl_cert_file<P: AsRef<Path>>(&mut self, file: P) -> &mut Self {
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
