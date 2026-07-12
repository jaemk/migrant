/*!
Connection settings
*/
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::errors::*;
use crate::macros::bail;
use crate::util::encode;
use crate::{DbKind, SQLITE_MEMORY_PATH};

use super::builders::{MySqlSettingsBuilder, PostgresSettingsBuilder, SqliteSettingsBuilder};

/// Resolve `env:VAR_NAME` values from the environment.
/// Missing environment variables resolve to an empty string.
fn resolve_env(value: &str) -> String {
    match value.strip_prefix("env:") {
        Some(var) => env::var(var).unwrap_or_default(),
        None => value.to_string(),
    }
}

fn resolve_env_opt(value: &Option<String>) -> Option<String> {
    value.as_deref().map(resolve_env)
}

/// Sqlite connection settings
#[derive(Deserialize, Debug, Clone)]
pub(crate) struct SqliteSettings {
    pub(crate) database_path: String,
    pub(crate) migration_location: Option<String>,
}

impl SqliteSettings {
    pub(crate) fn is_memory(&self) -> bool {
        self.database_path == SQLITE_MEMORY_PATH
    }

    fn resolve_env_vars(&self) -> Self {
        Self {
            database_path: resolve_env(&self.database_path),
            migration_location: resolve_env_opt(&self.migration_location),
        }
    }
}

/// Connection settings for server-based databases (postgres, mysql)
#[derive(Deserialize, Debug, Clone)]
pub(crate) struct ServerSettings {
    pub(crate) database_name: String,
    pub(crate) database_user: String,
    pub(crate) database_password: String,
    pub(crate) database_host: Option<String>,
    pub(crate) database_port: Option<String>,
    pub(crate) database_params: Option<BTreeMap<String, String>>,
    pub(crate) ssl_cert_file: Option<PathBuf>,
    pub(crate) migration_location: Option<String>,
}

impl ServerSettings {
    /// Build a `<scheme>://user:pass@host:port/db_name?params` url
    pub(crate) fn connect_string(&self, scheme: &str, default_port: &str) -> Result<String> {
        let non_empty_or = |val: &Option<String>, default: &str| -> String {
            match val.as_deref() {
                Some(v) if !v.is_empty() => v.to_string(),
                _ => default.to_string(),
            }
        };
        let host = encode(&non_empty_or(&self.database_host, "localhost"));
        let port = encode(&non_empty_or(&self.database_port, default_port));

        let s = format!(
            "{scheme}://{user}:{pass}@{host}:{port}/{db_name}",
            scheme = scheme,
            user = encode(&self.database_user),
            pass = encode(&self.database_password),
            host = host,
            port = port,
            db_name = encode(&self.database_name),
        );
        let mut url = url::Url::parse(&s)?;
        if let Some(ref params) = self.database_params {
            if !params.is_empty() {
                let mut pairs = url.query_pairs_mut();
                for (k, v) in params {
                    pairs.append_pair(&encode(k), &encode(v));
                }
            }
        }
        Ok(url.to_string())
    }

    fn resolve_env_vars(&self) -> Self {
        Self {
            database_name: resolve_env(&self.database_name),
            database_user: resolve_env(&self.database_user),
            database_password: resolve_env(&self.database_password),
            database_host: resolve_env_opt(&self.database_host),
            database_port: resolve_env_opt(&self.database_port),
            database_params: self.database_params.as_ref().map(|params| {
                params
                    .iter()
                    .map(|(k, v)| (k.clone(), resolve_env(v)))
                    .collect()
            }),
            ssl_cert_file: self.ssl_cert_file.clone(),
            migration_location: resolve_env_opt(&self.migration_location),
        }
    }
}

/// Settings for one of the supported databases
#[derive(Debug, Clone)]
pub(crate) enum DbSettings {
    Sqlite(SqliteSettings),
    Postgres(ServerSettings),
    MySql(ServerSettings),
}

impl DbSettings {
    pub(crate) fn db_kind(&self) -> DbKind {
        match self {
            DbSettings::Sqlite(_) => DbKind::Sqlite,
            DbSettings::Postgres(_) => DbKind::Postgres,
            DbSettings::MySql(_) => DbKind::MySql,
        }
    }

    pub(crate) fn migration_location(&self) -> Option<PathBuf> {
        let loc = match self {
            DbSettings::Sqlite(s) => &s.migration_location,
            DbSettings::Postgres(s) | DbSettings::MySql(s) => &s.migration_location,
        };
        loc.as_ref().map(PathBuf::from)
    }

    /// Is this an in-memory sqlite database?
    pub(crate) fn is_memory_sqlite(&self) -> bool {
        matches!(self, DbSettings::Sqlite(s) if s.is_memory())
    }

    pub(crate) fn database_path(&self) -> Result<PathBuf> {
        match self {
            DbSettings::Sqlite(s) => Ok(PathBuf::from(&s.database_path)),
            _ => bail!(
                Config,
                "Cannot generate database_path for database-type: {}",
                self.db_kind()
            ),
        }
    }

    pub(crate) fn connect_string(&self) -> Result<String> {
        match self {
            DbSettings::Postgres(s) => s.connect_string("postgres", "5432"),
            DbSettings::MySql(s) => s.connect_string("mysql", "3306"),
            DbSettings::Sqlite(_) => bail!(
                Config,
                "Cannot generate connect-string for database-type: {}",
                self.db_kind()
            ),
        }
    }

    pub(crate) fn ssl_cert_file(&self) -> Option<PathBuf> {
        match self {
            DbSettings::Postgres(s) => s.ssl_cert_file.clone(),
            _ => None,
        }
    }
}

/// Project settings
///
/// These settings are serialized and saved in a project `Migrant.toml` config file
/// or defined explicitly in source using the provided builder methods.
#[derive(Debug, Clone)]
pub struct Settings {
    pub(crate) inner: DbSettings,
}

impl Settings {
    pub(crate) fn new(inner: DbSettings) -> Self {
        Self { inner }
    }

    /// Initialize from a serialized settings file
    pub fn from_file<T: AsRef<Path>>(path: T) -> Result<Self> {
        #[derive(Deserialize)]
        struct DbTypeField {
            database_type: String,
        }
        let content = fs::read_to_string(path.as_ref())?;
        let type_field: DbTypeField = toml::from_str(&content)?;
        let inner = match type_field.database_type.as_str() {
            "sqlite" => {
                let settings: SqliteSettings = toml::from_str(&content)?;
                DbSettings::Sqlite(settings.resolve_env_vars())
            }
            "postgres" => {
                let settings: ServerSettings = toml::from_str(&content)?;
                DbSettings::Postgres(settings.resolve_env_vars())
            }
            "mysql" => {
                let settings: ServerSettings = toml::from_str(&content)?;
                DbSettings::MySql(settings.resolve_env_vars())
            }
            t => bail!(Config, "Invalid database_type: {:?}", t),
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
