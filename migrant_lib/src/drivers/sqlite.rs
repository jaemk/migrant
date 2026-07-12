/*!
Sqlite driver

The connection handle is kept alive (and shared between `Config` clones)
so that in-memory (`:memory:`) databases survive across operations.
*/
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::Connection;

use super::sql;
use crate::errors::*;
use crate::macros::err;

/// Path value indicating an in-memory sqlite database
pub(crate) const MEMORY_PATH: &str = ":memory:";

/// A live sqlite connection
///
/// The handle is reference counted so it can be shared with
/// function-migrations via [`ConnConfig`](crate::ConnConfig).
pub(crate) struct SqliteConn {
    handle: Arc<Mutex<Connection>>,
}

impl SqliteConn {
    /// Open a connection to a database file, or an in-memory database
    /// if the path is `:memory:`
    pub(crate) fn open(path: &str) -> Result<Self> {
        let conn = if path == MEMORY_PATH {
            Connection::open_in_memory()?
        } else {
            Connection::open(path)?
        };
        Ok(Self {
            handle: Arc::new(Mutex::new(conn)),
        })
    }

    /// Return a shared reference to the underlying connection
    pub(crate) fn handle(&self) -> Arc<Mutex<Connection>> {
        self.handle.clone()
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.handle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn migration_table_exists(&self) -> Result<bool> {
        let conn = self.lock();
        let exists: bool =
            conn.query_row(sql::SQLITE_MIGRATION_TABLE_EXISTS, [], |row| row.get(0))?;
        Ok(exists)
    }

    pub(crate) fn setup_migration_table(&self) -> Result<bool> {
        if self.migration_table_exists()? {
            return Ok(false);
        }
        self.lock().execute(sql::CREATE_TABLE, [])?;
        Ok(true)
    }

    pub(crate) fn applied_tags(&self) -> Result<Vec<String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(sql::GET_MIGRATIONS)?;
        let tags = stmt
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        Ok(tags)
    }

    pub(crate) fn insert_tag(&self, tag: &str) -> Result<()> {
        self.lock()
            .execute(sql::INSERT_MIGRATION_PG_SQLITE, [tag])?;
        Ok(())
    }

    pub(crate) fn remove_tag(&self, tag: &str) -> Result<()> {
        self.lock()
            .execute(sql::REMOVE_MIGRATION_PG_SQLITE, [tag])?;
        Ok(())
    }

    pub(crate) fn execute_batch(&self, stmt: &str) -> Result<()> {
        if stmt.is_empty() {
            return Ok(());
        }
        let conn = self.lock();
        let res = conn.execute_batch(stmt);
        if res.is_err() && !conn.is_autocommit() {
            // A failed batch may leave an open transaction on the shared
            // connection; roll it back so later operations aren't poisoned.
            let _ = conn.execute_batch("rollback");
        }
        res.map_err(|e| err!(Migration, "{}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_table_lifecycle() {
        let conn = SqliteConn::open(MEMORY_PATH).unwrap();

        assert!(
            !conn.migration_table_exists().unwrap(),
            "no table before setup"
        );
        assert!(conn.setup_migration_table().unwrap(), "table created");
        assert!(!conn.setup_migration_table().unwrap(), "setup idempotent");
        assert!(conn.migration_table_exists().unwrap(), "table exists");

        conn.insert_tag("initial").unwrap();
        conn.insert_tag("alter1").unwrap();
        conn.insert_tag("alter2").unwrap();
        assert_eq!(3, conn.applied_tags().unwrap().len());

        conn.remove_tag("alter2").unwrap();
        assert_eq!(2, conn.applied_tags().unwrap().len());

        conn.remove_tag("alter1").unwrap();
        conn.remove_tag("initial").unwrap();
        assert_eq!(0, conn.applied_tags().unwrap().len());
    }

    #[test]
    fn execute_batch_rolls_back_failed_transactions() {
        let conn = SqliteConn::open(MEMORY_PATH).unwrap();
        conn.execute_batch("create table t(x integer);").unwrap();
        let res = conn.execute_batch("begin; insert into t values (1); nonsense;");
        assert!(res.is_err());
        // connection must still be usable and outside a transaction
        conn.execute_batch("insert into t values (2);").unwrap();
        let count: i64 = conn
            .lock()
            .query_row("select count(*) from t", [], |row| row.get(0))
            .unwrap();
        assert_eq!(1, count, "failed batch was rolled back");
    }
}
