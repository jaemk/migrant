/*!
Sqlite driver

The connection handle is kept alive (and shared between `Config` clones)
so that in-memory (`:memory:`) databases survive across operations.
*/
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::Connection;

use super::{sql, AppliedRecord};
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
        self.lock().execute(sql::SQLITE_CREATE_TABLE, [])?;
        Ok(true)
    }

    pub(crate) fn applied_records(&self) -> Result<Vec<AppliedRecord>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(sql::GET_MIGRATIONS)?;
        let records = stmt
            .query_map([], |row| {
                Ok(AppliedRecord {
                    tag: row.get(0)?,
                    checksum: row.get(1)?,
                    repeatable: row.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<AppliedRecord>, _>>()?;
        Ok(records)
    }

    pub(crate) fn insert_tag(
        &self,
        tag: &str,
        checksum: Option<&str>,
        repeatable: bool,
    ) -> Result<()> {
        self.lock().execute(
            sql::INSERT_MIGRATION_PG_SQLITE,
            rusqlite::params![tag, checksum, repeatable],
        )?;
        Ok(())
    }

    pub(crate) fn update_tag(&self, tag: &str, checksum: Option<&str>) -> Result<()> {
        self.lock().execute(
            sql::UPDATE_MIGRATION_SQLITE,
            rusqlite::params![checksum, tag],
        )?;
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

    pub(crate) fn begin(&self) -> Result<()> {
        self.lock()
            .execute_batch("begin")
            .map_err(|e| err!(Migration, "{}", e))
    }

    pub(crate) fn commit(&self) -> Result<()> {
        self.lock()
            .execute_batch("commit")
            .map_err(|e| err!(Migration, "{}", e))
    }

    pub(crate) fn rollback(&self) -> Result<()> {
        self.lock()
            .execute_batch("rollback")
            .map_err(|e| err!(Migration, "{}", e))
    }

    /// Sqlite has no cross-process advisory lock (a single connection already
    /// serializes writers, and there is no server to coordinate through), so
    /// migration-run locking is a no-op.
    pub(crate) fn acquire_lock(&self) -> Result<()> {
        Ok(())
    }

    pub(crate) fn release_lock(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(tag: &str, checksum: Option<&str>, repeatable: bool) -> AppliedRecord {
        AppliedRecord {
            tag: tag.to_string(),
            checksum: checksum.map(str::to_string),
            repeatable,
        }
    }

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

        conn.insert_tag("initial", Some("abc123"), false).unwrap();
        conn.insert_tag("alter1", None, false).unwrap();
        conn.insert_tag("alter2", Some("def456"), false).unwrap();
        // Recorded order is authoritative: records come back in insertion (id)
        // order, each carrying its tag and checksum (NULL where None).
        assert_eq!(
            vec![
                record("initial", Some("abc123"), false),
                record("alter1", None, false),
                record("alter2", Some("def456"), false),
            ],
            conn.applied_records().unwrap()
        );

        // `applied_at` is populated by the column default.
        let stamped: i64 = conn
            .lock()
            .query_row(
                "select count(*) from __migrant_migrations where applied_at is not null",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(3, stamped);

        conn.remove_tag("alter2").unwrap();
        assert_eq!(2, conn.applied_records().unwrap().len());

        conn.remove_tag("alter1").unwrap();
        conn.remove_tag("initial").unwrap();
        assert_eq!(0, conn.applied_records().unwrap().len());
    }

    // REPEAT-6
    #[test]
    fn repeatable_rows_update_in_place_keeping_their_id() {
        let conn = SqliteConn::open(MEMORY_PATH).unwrap();
        conn.setup_migration_table().unwrap();

        conn.insert_tag("initial", Some("abc123"), false).unwrap();
        conn.insert_tag("seed-roles", Some("sum-v1"), true).unwrap();
        conn.insert_tag("later", Some("def456"), false).unwrap();

        // The repeatable row records its kind.
        assert_eq!(
            vec![
                record("initial", Some("abc123"), false),
                record("seed-roles", Some("sum-v1"), true),
                record("later", Some("def456"), false),
            ],
            conn.applied_records().unwrap()
        );

        let id_before: i64 = conn
            .lock()
            .query_row(
                "select id from __migrant_migrations where tag = 'seed-roles'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        conn.update_tag("seed-roles", Some("sum-v2")).unwrap();

        // One row still, with the new checksum, still marked repeatable, and
        // still in the same position in recorded order.
        assert_eq!(
            vec![
                record("initial", Some("abc123"), false),
                record("seed-roles", Some("sum-v2"), true),
                record("later", Some("def456"), false),
            ],
            conn.applied_records().unwrap()
        );
        let id_after: i64 = conn
            .lock()
            .query_row(
                "select id from __migrant_migrations where tag = 'seed-roles'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            id_before, id_after,
            "an in-place re-run must not change the row's id"
        );

        // A row first recorded for a versioned migration is marked repeatable
        // by the update, so a migration converted from versioned to repeatable
        // ends up with a truthful row rather than a stale `false`.
        conn.update_tag("later", Some("ghi789")).unwrap();
        assert_eq!(
            record("later", Some("ghi789"), true),
            conn.applied_records().unwrap()[2],
            "an in-place update marks the row repeatable"
        );
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
