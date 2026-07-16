/*!
Database drivers

Each enabled backend provides a connection type holding a live database
connection. All migration-table operations go through [`DbConnection`],
which is lazily established by [`Config`](crate::Config) and kept alive
for the life of the config (and all of its clones).
*/
use std::fmt;

use crate::config::Config;
use crate::errors::*;
use crate::DbKind;

#[allow(dead_code)] // per-backend statements are unused when their feature is disabled
pub(crate) mod sql {
    pub static CREATE_TABLE: &str = "create table __migrant_migrations(tag text unique);";
    pub static MYSQL_CREATE_TABLE: &str =
        "create table __migrant_migrations(tag varchar(512) unique);";

    pub static GET_MIGRATIONS: &str = "select tag from __migrant_migrations;";
    pub static INSERT_MIGRATION_PG_SQLITE: &str =
        "insert into __migrant_migrations (tag) values ($1)";
    pub static REMOVE_MIGRATION_PG_SQLITE: &str = "delete from __migrant_migrations where tag = $1";
    pub static INSERT_MIGRATION_MYSQL: &str = "insert into __migrant_migrations (tag) values (?)";
    pub static REMOVE_MIGRATION_MYSQL: &str = "delete from __migrant_migrations where tag = ?";

    pub static SQLITE_MIGRATION_TABLE_EXISTS: &str = "select exists(select 1 from sqlite_master where type = 'table' and name = '__migrant_migrations');";
    pub static PG_MIGRATION_TABLE_EXISTS: &str =
        "select exists(select 1 from pg_tables where tablename = '__migrant_migrations');";
    pub static MYSQL_MIGRATION_TABLE_EXISTS: &str = "select exists(select 1 from information_schema.tables where table_name='__migrant_migrations' and table_schema = database()) as tag;";
}

#[cfg(feature = "mysql")]
pub(crate) mod mysql;
#[cfg(feature = "postgres")]
pub(crate) mod pg;
#[cfg(feature = "sqlite")]
pub(crate) mod sqlite;

/// A live connection to one of the supported databases
///
/// Server connections are boxed to keep the enum small
pub(crate) enum DbConnection {
    #[cfg(feature = "sqlite")]
    Sqlite(sqlite::SqliteConn),
    #[cfg(feature = "postgres")]
    Postgres(Box<pg::PgConn>),
    #[cfg(feature = "mysql")]
    MySql(Box<mysql::MySqlConn>),
}

impl fmt::Debug for DbConnection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let kind = match self {
            #[cfg(feature = "sqlite")]
            DbConnection::Sqlite(_) => "sqlite",
            #[cfg(feature = "postgres")]
            DbConnection::Postgres(_) => "postgres",
            #[cfg(feature = "mysql")]
            DbConnection::MySql(_) => "mysql",
            #[allow(unreachable_patterns)]
            _ => "unknown",
        };
        write!(f, "DbConnection({})", kind)
    }
}

/// Dispatch a method call to the active backend connection
macro_rules! dispatch {
    ($self:expr, $conn:ident => $body:expr) => {
        match $self {
            #[cfg(feature = "sqlite")]
            DbConnection::Sqlite($conn) => $body,
            #[cfg(feature = "postgres")]
            DbConnection::Postgres($conn) => $body,
            #[cfg(feature = "mysql")]
            DbConnection::MySql($conn) => $body,
            #[allow(unreachable_patterns)]
            _ => Err(Error::FeatureRequired("sqlite / postgres / mysql")),
        }
    };
}

// method arguments are unused in the fallback arm when no db features are enabled
#[allow(unused_variables)]
impl DbConnection {
    /// Open a new connection for the given config
    pub(crate) fn connect(config: &Config) -> Result<Self> {
        match config.database_type() {
            DbKind::Sqlite => {
                #[cfg(feature = "sqlite")]
                {
                    let path = config.database_path_string()?;
                    Ok(DbConnection::Sqlite(sqlite::SqliteConn::open(&path)?))
                }
                #[cfg(not(feature = "sqlite"))]
                Err(Error::FeatureRequired("sqlite"))
            }
            DbKind::Postgres => {
                #[cfg(feature = "postgres")]
                {
                    let conn_str = config.connect_string()?;
                    let cert = config.ssl_cert_file();
                    Ok(DbConnection::Postgres(Box::new(pg::PgConn::connect(
                        &conn_str,
                        cert.as_deref(),
                    )?)))
                }
                #[cfg(not(feature = "postgres"))]
                Err(Error::FeatureRequired("postgres"))
            }
            DbKind::MySql => {
                #[cfg(feature = "mysql")]
                {
                    let conn_str = config.connect_string()?;
                    Ok(DbConnection::MySql(Box::new(mysql::MySqlConn::connect(
                        &conn_str,
                    )?)))
                }
                #[cfg(not(feature = "mysql"))]
                Err(Error::FeatureRequired("mysql"))
            }
        }
    }

    /// Check whether the `__migrant_migrations` table exists
    pub(crate) fn migration_table_exists(&mut self) -> Result<bool> {
        dispatch!(self, c => c.migration_table_exists())
    }

    /// Create the `__migrant_migrations` table if missing, returning `true` if created
    pub(crate) fn setup_migration_table(&mut self) -> Result<bool> {
        dispatch!(self, c => c.setup_migration_table())
    }

    /// Select all applied migration tags
    pub(crate) fn applied_tags(&mut self) -> Result<Vec<String>> {
        dispatch!(self, c => c.applied_tags())
    }

    /// Record a migration tag as applied
    pub(crate) fn insert_tag(&mut self, tag: &str) -> Result<()> {
        dispatch!(self, c => c.insert_tag(tag))
    }

    /// Remove a migration tag from the applied set
    pub(crate) fn remove_tag(&mut self, tag: &str) -> Result<()> {
        dispatch!(self, c => c.remove_tag(tag))
    }

    /// Execute a batch of sql statements
    pub(crate) fn execute_batch(&mut self, sql: &str) -> Result<()> {
        dispatch!(self, c => c.execute_batch(sql))
    }

    /// Begin a transaction on this connection
    pub(crate) fn begin(&mut self) -> Result<()> {
        dispatch!(self, c => c.begin())
    }

    /// Commit the current transaction on this connection
    pub(crate) fn commit(&mut self) -> Result<()> {
        dispatch!(self, c => c.commit())
    }

    /// Roll back the current transaction on this connection
    pub(crate) fn rollback(&mut self) -> Result<()> {
        dispatch!(self, c => c.rollback())
    }

    /// Acquire the session-level advisory lock that serializes migration runs.
    ///
    /// Blocks until the lock is available. Sqlite has no advisory lock (and no
    /// cross-process migration concurrency to guard against), so it is a no-op.
    pub(crate) fn acquire_lock(&mut self) -> Result<()> {
        dispatch!(self, c => c.acquire_lock())
    }

    /// Release the session-level advisory lock. No-op for sqlite.
    pub(crate) fn release_lock(&mut self) -> Result<()> {
        dispatch!(self, c => c.release_lock())
    }
}

#[cfg(test)]
mod tests {
    use super::sql;

    /// Regression guard for F2: the mysql migration-table-exists check must be
    /// scoped to the current database via `table_schema = database()`, otherwise
    /// a `__migrant_migrations` table in any other schema on the server is a
    /// false positive and setup skips creating the table in the right schema.
    #[test]
    fn mysql_migration_table_exists_is_scoped_to_current_schema() {
        assert!(
            sql::MYSQL_MIGRATION_TABLE_EXISTS.contains("table_schema = database()"),
            "MYSQL_MIGRATION_TABLE_EXISTS must filter on table_schema = database(): {}",
            sql::MYSQL_MIGRATION_TABLE_EXISTS
        );
    }
}
