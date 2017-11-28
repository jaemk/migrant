///! Database migration connection
use {Config};
use errors::*;

#[cfg(feature="postgresql")]
use postgres;

#[cfg(feature="sqlite")]
use rusqlite;


#[allow(dead_code)]
pub mod markers {
    pub struct PostgresqlFeatureRequired;
    pub struct SqliteFeatureRequired;
}
#[allow(unused_imports)]
use self::markers::*;


/// Database connection wrapper
#[allow(dead_code)]
pub struct DbConn<'a> {
    config: &'a Config,
}
impl<'a> DbConn<'a> {
    pub fn new(config: &'a Config) -> Self {
        Self { config }
    }

    /// Generate a `postgres::Connection`, `postgresql` feature required
    #[cfg(not(feature="postgresql"))]
    pub fn pg_connection(&self) -> Result<PostgresqlFeatureRequired> {
        unimplemented!()
    }

    /// Generate a `postgres::Connection`, `postgresql` feature required
    #[cfg(feature="postgresql")]
    pub fn pg_connection(&self) -> Result<postgres::Connection> {
        let conn_str = self.config.connect_string()?;
        Ok(postgres::Connection::connect(conn_str, postgres::TlsMode::None)?)
    }

    /// Generate a `rusqlite::Connection`, `sqlite` feature required
    #[cfg(not(feature="sqlite"))]
    pub fn sqlite_connection(&self) -> Result<SqliteFeatureRequired> {
        unimplemented!()
    }

    /// Generate a `rusqlite::Connection`, `sqlite` feature required
    #[cfg(feature="sqlite")]
    pub fn sqlite_connection(&self) -> Result<rusqlite::Connection> {
        let db_path = self.config.database_path()?;
        Ok(rusqlite::Connection::open(db_path)?)
    }
}

