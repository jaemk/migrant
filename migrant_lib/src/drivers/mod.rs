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
    // The bookkeeping table carries, besides the migration `tag`: a surrogate
    // `id` whose ascending order is the authoritative recorded application
    // order, an optional `checksum` (lowercase hex sha256 of the raw up-direction
    // SQL, NULL for programmatic migrations that have no SQL to hash), an
    // `applied_at` timestamp populated by the column default, and `is_repeatable`
    // marking rows that record a repeatable migration (re-run on every checksum
    // change). The column is named `is_repeatable` rather than `repeatable`
    // because the latter is a keyword on both postgres and mysql.
    pub static PG_CREATE_TABLE: &str = "create table __migrant_migrations(id serial primary key, tag text unique not null, checksum text, applied_at timestamptz not null default now(), is_repeatable boolean not null default false);";
    // Sqlite uses `0`/`1` rather than the `false`/`true` keywords, which its
    // parser only gained in 3.23.0. `migrant_lib` links whatever libsqlite3 the
    // consumer provides (its rusqlite dependency is not `bundled`), so the
    // portable spelling is the safe one.
    pub static SQLITE_CREATE_TABLE: &str = "create table __migrant_migrations(id integer primary key autoincrement, tag text unique not null, checksum text, applied_at timestamp not null default current_timestamp, is_repeatable boolean not null default 0);";
    pub static MYSQL_CREATE_TABLE: &str = "create table __migrant_migrations(id integer primary key auto_increment, tag varchar(512) unique not null, checksum text null, applied_at timestamp not null default current_timestamp, is_repeatable boolean not null default false);";

    // Recorded application order is authoritative, so order by the surrogate id.
    // Each row carries the tag, its recorded checksum (NULL for programmatic
    // migrations), and whether it records a repeatable migration. Checksum drift
    // of an already-applied versioned migration is detected by comparing the
    // recorded checksum against the migration's current one; for a repeatable
    // row the same difference is instead the signal to re-run.
    pub static GET_MIGRATIONS: &str =
        "select tag, checksum, is_repeatable from __migrant_migrations order by id;";
    pub static INSERT_MIGRATION_PG_SQLITE: &str =
        "insert into __migrant_migrations (tag, checksum, is_repeatable) values ($1, $2, $3)";
    pub static REMOVE_MIGRATION_PG_SQLITE: &str = "delete from __migrant_migrations where tag = $1";
    pub static INSERT_MIGRATION_MYSQL: &str =
        "insert into __migrant_migrations (tag, checksum, is_repeatable) values (?, ?, ?)";
    pub static REMOVE_MIGRATION_MYSQL: &str = "delete from __migrant_migrations where tag = ?";

    // A repeatable migration keeps one row, updated in place on each re-run, so
    // its `id` (and therefore recorded application order) is preserved.
    // `applied_at` is refreshed explicitly because the column default only
    // applies on insert. `is_repeatable` is set rather than left alone so a row
    // first recorded for a versioned migration becomes correctly marked once
    // that migration is declared repeatable; only a repeatable re-run takes
    // this path.
    pub static UPDATE_MIGRATION_PG: &str = "update __migrant_migrations set checksum = $1, applied_at = now(), is_repeatable = true where tag = $2";
    pub static UPDATE_MIGRATION_SQLITE: &str = "update __migrant_migrations set checksum = $1, applied_at = current_timestamp, is_repeatable = 1 where tag = $2";
    pub static UPDATE_MIGRATION_MYSQL: &str = "update __migrant_migrations set checksum = ?, applied_at = current_timestamp, is_repeatable = true where tag = ?";

    pub static SQLITE_MIGRATION_TABLE_EXISTS: &str = "select exists(select 1 from sqlite_master where type = 'table' and name = '__migrant_migrations');";
    pub static PG_MIGRATION_TABLE_EXISTS: &str =
        "select exists(select 1 from pg_tables where tablename = '__migrant_migrations');";
    pub static MYSQL_MIGRATION_TABLE_EXISTS: &str = "select exists(select 1 from information_schema.tables where table_name='__migrant_migrations' and table_schema = database()) as tag;";
}

/// One row of the `__migrant_migrations` bookkeeping table, in recorded
/// application order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AppliedRecord {
    /// The recorded migration tag
    pub(crate) tag: String,
    /// The checksum recorded when the migration was applied, `None` where the
    /// column is NULL (programmatic migrations, or legacy rows)
    pub(crate) checksum: Option<String>,
    /// Whether the row records a repeatable migration
    pub(crate) repeatable: bool,
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

    /// Select all applied migrations as [`AppliedRecord`]s, in recorded
    /// application order.
    pub(crate) fn applied_records(&mut self) -> Result<Vec<AppliedRecord>> {
        dispatch!(self, c => c.applied_records())
    }

    /// Record a migration tag as applied, along with its optional checksum and
    /// whether it is repeatable (`applied_at` is populated by the column
    /// default)
    pub(crate) fn insert_tag(
        &mut self,
        tag: &str,
        checksum: Option<&str>,
        repeatable: bool,
    ) -> Result<()> {
        dispatch!(self, c => c.insert_tag(tag, checksum, repeatable))
    }

    /// Update an already-recorded tag's checksum and `applied_at` in place, and
    /// mark it repeatable. Used when a repeatable migration re-runs, so the row
    /// keeps its `id` and recorded application order is unchanged.
    pub(crate) fn update_tag(&mut self, tag: &str, checksum: Option<&str>) -> Result<()> {
        dispatch!(self, c => c.update_tag(tag, checksum))
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

    /// Every backend's create-table statement must carry the `tag`, `checksum`,
    /// `applied_at`, and `is_repeatable` columns, and applied tags must be
    /// selected in recorded order (`order by id`) so the recorded application
    /// order is authoritative.
    #[test]
    fn create_table_statements_carry_every_bookkeeping_column() {
        for (name, ddl) in [
            ("pg", sql::PG_CREATE_TABLE),
            ("sqlite", sql::SQLITE_CREATE_TABLE),
            ("mysql", sql::MYSQL_CREATE_TABLE),
        ] {
            for column in ["tag", "checksum", "applied_at", "is_repeatable"] {
                assert!(
                    ddl.contains(column),
                    "{name} ddl must have a {column} column: {ddl}"
                );
            }
        }
        assert!(
            sql::GET_MIGRATIONS.contains("order by id"),
            "GET_MIGRATIONS must order by id: {}",
            sql::GET_MIGRATIONS
        );
        for column in ["tag", "checksum", "is_repeatable"] {
            assert!(
                sql::GET_MIGRATIONS.contains(column),
                "GET_MIGRATIONS must select {column}: {}",
                sql::GET_MIGRATIONS
            );
            assert!(
                sql::INSERT_MIGRATION_PG_SQLITE.contains(column)
                    && sql::INSERT_MIGRATION_MYSQL.contains(column),
                "inserts must carry the {column} column"
            );
        }
    }

    /// REPEAT-6: every backend's in-place update must refresh `checksum` and
    /// `applied_at`, mark the row repeatable, and be scoped to one tag. Without
    /// the `where tag` clause it would rewrite the whole table.
    #[test]
    fn update_statements_refresh_the_row_in_place_for_one_tag() {
        for (name, update) in [
            ("pg", sql::UPDATE_MIGRATION_PG),
            ("sqlite", sql::UPDATE_MIGRATION_SQLITE),
            ("mysql", sql::UPDATE_MIGRATION_MYSQL),
        ] {
            assert!(
                update.contains("checksum ="),
                "{name} update must set checksum: {update}"
            );
            assert!(
                update.contains("applied_at ="),
                "{name} update must refresh applied_at: {update}"
            );
            assert!(
                update.contains("is_repeatable ="),
                "{name} update must mark the row repeatable: {update}"
            );
            assert!(
                update.contains("where tag ="),
                "{name} update must be scoped to a single tag: {update}"
            );
        }
        // The sqlite parser only gained the `true`/`false` keywords in 3.23.0,
        // and `migrant_lib` links the consumer's libsqlite3.
        assert!(
            !sql::SQLITE_CREATE_TABLE.contains("false")
                && !sql::UPDATE_MIGRATION_SQLITE.contains("true"),
            "sqlite statements must use 0/1 rather than the boolean keywords"
        );
    }
}
