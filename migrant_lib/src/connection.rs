/*!
Database connection information passed to function-migrations
*/
use crate::errors::*;
use crate::{Config, DbKind};

/// Database connection information
///
/// Passed to `FnMigration` functions so they can connect to (or reuse a
/// connection to) the database being migrated.
pub struct ConnConfig<'a> {
    config: &'a Config,
}

impl<'a> ConnConfig<'a> {
    pub(crate) fn new(config: &'a Config) -> Self {
        Self { config }
    }

    /// Return the database type
    pub fn database_type(&self) -> DbKind {
        self.config.database_type()
    }

    /// Return a connection string for postgres or mysql
    pub fn connect_string(&self) -> Result<String> {
        self.config.connect_string()
    }

    /// Return a sqlite database path.
    /// In-memory databases return `:memory:`; use
    /// [`sqlite_connection`](ConnConfig::sqlite_connection) to operate on them.
    pub fn database_path(&self) -> Result<std::path::PathBuf> {
        self.config.database_path()
    }

    /// Return a shared handle to the live sqlite connection.
    ///
    /// This is the same connection used to apply migrations, so it works for
    /// in-memory (`:memory:`) databases as well as file-backed ones.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # #[cfg(feature = "sqlite")]
    /// fn add_data(config: migrant_lib::ConnConfig) -> Result<(), Box<dyn std::error::Error>> {
    ///     let conn = config.sqlite_connection()?;
    ///     let conn = conn.lock().unwrap();
    ///     conn.execute("insert into users (name) values (?1)", ["me"])?;
    ///     Ok(())
    /// }
    /// ```
    #[cfg(feature = "sqlite")]
    pub fn sqlite_connection(
        &self,
    ) -> Result<std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>> {
        self.config.sqlite_connection()
    }
}
