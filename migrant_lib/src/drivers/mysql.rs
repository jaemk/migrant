/*!
MySQL driver
*/
use mysql::{prelude::Queryable, Conn, Opts};

use super::sql;
use crate::errors::*;
use crate::macros::{bail, err};

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
}
