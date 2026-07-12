/*!
Postgres driver
*/
use std::path::Path;

use postgres::{Client, NoTls};

use super::sql;
use crate::errors::*;
use crate::macros::err;

/// A live postgres connection
pub(crate) struct PgConn {
    client: Client,
}

fn tls_connector(cert: &Path) -> Result<postgres_native_tls::MakeTlsConnector> {
    let cert =
        std::fs::read(cert).map_err(|e| err!(Migration, "postgres cert file error {}", e))?;
    let cert = native_tls::Certificate::from_pem(&cert)
        .map_err(|e| err!(Migration, "postgres cert load error {}", e))?;
    let connector = native_tls::TlsConnector::builder()
        .add_root_certificate(cert)
        .build()
        .map_err(|e| err!(Migration, "postgres tls-connection error {}", e))?;
    Ok(postgres_native_tls::MakeTlsConnector::new(connector))
}

impl PgConn {
    /// Connect, optionally verifying the server with a custom ssl cert
    pub(crate) fn connect(conn_str: &str, cert: Option<&Path>) -> Result<Self> {
        let client = match cert {
            None => Client::connect(conn_str, NoTls)?,
            Some(cert) => Client::connect(conn_str, tls_connector(cert)?)?,
        };
        Ok(Self { client })
    }

    pub(crate) fn migration_table_exists(&mut self) -> Result<bool> {
        let row = self.client.query_one(sql::PG_MIGRATION_TABLE_EXISTS, &[])?;
        Ok(row.get(0))
    }

    pub(crate) fn setup_migration_table(&mut self) -> Result<bool> {
        if self.migration_table_exists()? {
            return Ok(false);
        }
        self.client.execute(sql::CREATE_TABLE, &[])?;
        Ok(true)
    }

    pub(crate) fn applied_tags(&mut self) -> Result<Vec<String>> {
        let rows = self.client.query(sql::GET_MIGRATIONS, &[])?;
        Ok(rows.iter().map(|row| row.get(0)).collect())
    }

    pub(crate) fn insert_tag(&mut self, tag: &str) -> Result<()> {
        self.client
            .execute(sql::INSERT_MIGRATION_PG_SQLITE, &[&tag])?;
        Ok(())
    }

    pub(crate) fn remove_tag(&mut self, tag: &str) -> Result<()> {
        self.client
            .execute(sql::REMOVE_MIGRATION_PG_SQLITE, &[&tag])?;
        Ok(())
    }

    pub(crate) fn execute_batch(&mut self, stmt: &str) -> Result<()> {
        if stmt.is_empty() {
            return Ok(());
        }
        self.client
            .batch_execute(stmt)
            .map_err(|e| err!(Migration, "{}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Requires a running postgres instance; set `POSTGRES_TEST_CONN_STR`
    /// (e.g. `postgres://user:pass@localhost/db`) to run
    #[test]
    fn migration_table_lifecycle() {
        let conn_str = match std::env::var("POSTGRES_TEST_CONN_STR") {
            Ok(s) => s,
            Err(_) => {
                eprintln!("POSTGRES_TEST_CONN_STR not set, skipping");
                return;
            }
        };
        let mut conn = PgConn::connect(&conn_str, None).unwrap();

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
