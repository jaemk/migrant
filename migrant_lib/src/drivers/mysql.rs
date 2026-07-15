/*!
MySQL driver
*/
use mysql::{prelude::Queryable, Conn, Opts};

use super::sql;
use crate::errors::*;
use crate::macros::{bail, err};

/// Named advisory lock that serializes concurrent migration runs.
///
/// MySQL `GET_LOCK`/`RELEASE_LOCK` are keyed by name and scoped to the session,
/// so the lock is released automatically if this connection drops. The name is
/// arbitrary but must be identical across every process using this library.
const ADVISORY_LOCK_NAME: &str = "__migrant_migrations";

/// A live mysql connection
pub(crate) struct MySqlConn {
    conn: Conn,
}

impl MySqlConn {
    pub(crate) fn connect(conn_str: &str) -> Result<Self> {
        let opts = Opts::from_url(conn_str)
            .map_err(|e| err!(Config, "Error parsing mysql connection string: {}", e))?;
        let conn = Conn::new(opts)?;
        Ok(Self { conn })
    }

    pub(crate) fn migration_table_exists(&mut self) -> Result<bool> {
        let rows: Vec<u32> = self.conn.query(sql::MYSQL_MIGRATION_TABLE_EXISTS)?;
        if rows.len() != 1 {
            bail!(
                Migration,
                "Migration table check: expected 1 returned row, got {}",
                rows.len()
            )
        }
        Ok(rows[0] == 1)
    }

    pub(crate) fn setup_migration_table(&mut self) -> Result<bool> {
        if self.migration_table_exists()? {
            return Ok(false);
        }
        self.conn.query_drop(sql::MYSQL_CREATE_TABLE)?;
        Ok(true)
    }

    pub(crate) fn applied_tags(&mut self) -> Result<Vec<String>> {
        Ok(self.conn.query(sql::GET_MIGRATIONS)?)
    }

    pub(crate) fn insert_tag(&mut self, tag: &str) -> Result<()> {
        self.conn.exec_drop(sql::INSERT_MIGRATION_MYSQL, (tag,))?;
        Ok(())
    }

    pub(crate) fn remove_tag(&mut self, tag: &str) -> Result<()> {
        self.conn.exec_drop(sql::REMOVE_MIGRATION_MYSQL, (tag,))?;
        Ok(())
    }

    pub(crate) fn execute_batch(&mut self, stmt: &str) -> Result<()> {
        if stmt.is_empty() {
            return Ok(());
        }
        self.conn
            .query_drop(stmt)
            .map_err(|e| err!(Migration, "{}", e))
    }

    pub(crate) fn begin(&mut self) -> Result<()> {
        self.conn
            .query_drop("begin")
            .map_err(|e| err!(Migration, "{}", e))
    }

    pub(crate) fn commit(&mut self) -> Result<()> {
        self.conn
            .query_drop("commit")
            .map_err(|e| err!(Migration, "{}", e))
    }

    pub(crate) fn rollback(&mut self) -> Result<()> {
        self.conn
            .query_drop("rollback")
            .map_err(|e| err!(Migration, "{}", e))
    }

    /// Take the named advisory lock, blocking until it is available (a negative
    /// `GET_LOCK` timeout waits indefinitely). MySQL releases it automatically
    /// if this connection (session) drops.
    pub(crate) fn acquire_lock(&mut self) -> Result<()> {
        let got: Option<Option<i64>> = self
            .conn
            .query_first(format!("select get_lock('{}', -1)", ADVISORY_LOCK_NAME))
            .map_err(|e| err!(Migration, "{}", e))?;
        match got {
            Some(Some(1)) => Ok(()),
            _ => bail!(
                Migration,
                "could not acquire mysql advisory lock `{}`",
                ADVISORY_LOCK_NAME
            ),
        }
    }

    pub(crate) fn release_lock(&mut self) -> Result<()> {
        self.conn
            .query_drop(format!("select release_lock('{}')", ADVISORY_LOCK_NAME))
            .map_err(|e| err!(Migration, "{}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Requires a running mysql instance; set `MYSQL_TEST_CONN_STR`
    /// (e.g. `mysql://user:pass@localhost/db`) to run
    #[test]
    fn migration_table_lifecycle() {
        let conn_str = match std::env::var("MYSQL_TEST_CONN_STR") {
            Ok(s) => s,
            Err(_) => {
                eprintln!("MYSQL_TEST_CONN_STR not set, skipping");
                return;
            }
        };
        let mut conn = MySqlConn::connect(&conn_str).unwrap();

        // drop any leftover table from an earlier interrupted run
        conn.execute_batch("drop table if exists __migrant_migrations;")
            .unwrap();

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

        conn.execute_batch("drop table __migrant_migrations;")
            .unwrap();
    }

    /// Advisory-lock behavior. The two scenarios share the one fixed lock
    /// name, so they run sequentially inside a single `#[test]` rather than
    /// racing each other under cargo's parallel runner.
    /// Requires a running mysql instance (`MYSQL_TEST_CONN_STR`).
    #[test]
    fn advisory_lock() {
        let conn_str = match std::env::var("MYSQL_TEST_CONN_STR") {
            Ok(s) => s,
            Err(_) => {
                eprintln!("MYSQL_TEST_CONN_STR not set, skipping");
                return;
            }
        };
        lock_is_exclusive(&conn_str);
        lock_survives_in_transaction_error(&conn_str);
    }

    /// `get_lock` with a zero timeout returns immediately: 1 got it, 0 timed out.
    fn try_lock(c: &mut MySqlConn) -> Option<i64> {
        c.conn
            .query_first(format!("select get_lock('{}', 0)", ADVISORY_LOCK_NAME))
            .unwrap()
            .flatten()
    }

    /// While one session holds the lock, another cannot, and it becomes
    /// available again once released.
    fn lock_is_exclusive(conn_str: &str) {
        let mut holder = MySqlConn::connect(conn_str).unwrap();
        let mut other = MySqlConn::connect(conn_str).unwrap();

        holder.acquire_lock().unwrap();
        assert_eq!(
            Some(0),
            try_lock(&mut other),
            "second session must not acquire a held lock"
        );
        holder.release_lock().unwrap();
        assert_eq!(
            Some(1),
            try_lock(&mut other),
            "lock must be available once released"
        );
        other.release_lock().unwrap();
    }

    /// The session-scoped lock survives a failed statement inside an explicit
    /// transaction followed by the in-place rollback recovery `with_conn`
    /// performs on server errors.
    fn lock_survives_in_transaction_error(conn_str: &str) {
        let mut holder = MySqlConn::connect(conn_str).unwrap();
        let mut other = MySqlConn::connect(conn_str).unwrap();

        holder.acquire_lock().unwrap();
        // Provoke an error inside an explicit transaction.
        holder.begin().unwrap();
        assert!(holder
            .execute_batch("select * from does_not_exist")
            .is_err());
        // Recover in place, exactly as `with_conn` does on a server error.
        holder.rollback().unwrap();

        // The session survived, so it still holds the lock.
        assert_eq!(
            Some(0),
            try_lock(&mut other),
            "advisory lock must survive an in-transaction error"
        );
        // The recovered connection is usable again.
        let one: Option<i64> = holder.conn.query_first("select 1").unwrap();
        assert_eq!(Some(1), one);

        holder.release_lock().unwrap();
        other.release_lock().unwrap();
    }
}
