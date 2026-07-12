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

/// Build a TLS connector that trusts the system's root certificates.
fn system_tls_connector() -> Result<postgres_native_tls::MakeTlsConnector> {
    let connector = native_tls::TlsConnector::new()
        .map_err(|e| err!(Migration, "postgres tls-connection error {}", e))?;
    Ok(postgres_native_tls::MakeTlsConnector::new(connector))
}

/// Extract the `sslmode` param value (if any) from a postgres connection
/// string, handling both URL-style query params (`?sslmode=require`) and
/// libpq keyword/value strings (`sslmode=require`).
fn sslmode_value(conn_str: &str) -> Option<&str> {
    conn_str
        .split(|c: char| c.is_whitespace() || matches!(c, '?' | '&' | ';'))
        .find_map(|token| {
            let (key, value) = token.split_once('=')?;
            key.trim().eq_ignore_ascii_case("sslmode").then_some(value)
        })
        .map(str::trim)
}

/// Decide whether a connection string requests TLS.
///
/// Returns `false` when no `sslmode` param is present or it is `disable`
/// (the historical default), and `true` for any other value
/// (`prefer`/`require`/`verify-ca`/`verify-full`).
fn conn_str_wants_tls(conn_str: &str) -> bool {
    match sslmode_value(conn_str) {
        None => false,
        Some(mode) => !mode.eq_ignore_ascii_case("disable"),
    }
}

impl PgConn {
    /// Connect to postgres, selecting a TLS backend from the connection string.
    ///
    /// - When a custom `cert` file is given, the server is verified against
    ///   that root certificate.
    /// - Otherwise the connection string's `sslmode` param decides: absent or
    ///   `disable` connects without TLS (the default); any other value
    ///   (`prefer`/`require`/`verify-ca`/`verify-full`) connects with a
    ///   `native-tls` connector using the system trust roots.
    pub(crate) fn connect(conn_str: &str, cert: Option<&Path>) -> Result<Self> {
        let client = match cert {
            Some(cert) => Client::connect(conn_str, tls_connector(cert)?)?,
            None if conn_str_wants_tls(conn_str) => {
                Client::connect(conn_str, system_tls_connector()?)?
            }
            None => Client::connect(conn_str, NoTls)?,
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

    #[test]
    fn sslmode_value_parsing() {
        assert_eq!(sslmode_value("postgres://user:pass@localhost/db"), None);
        assert_eq!(
            sslmode_value("postgres://user:pass@localhost/db?sslmode=require"),
            Some("require")
        );
        assert_eq!(
            sslmode_value("postgres://u:p@h/db?connect_timeout=10&sslmode=verify-full"),
            Some("verify-full")
        );
        assert_eq!(
            sslmode_value("host=localhost user=u sslmode=disable dbname=db"),
            Some("disable")
        );
        assert_eq!(sslmode_value("host=localhost user=u dbname=db"), None);
    }

    #[test]
    fn conn_str_wants_tls_decision() {
        // no sslmode param -> unchanged default, no TLS
        assert!(!conn_str_wants_tls("postgres://user:pass@localhost/db"));
        assert!(!conn_str_wants_tls("host=localhost user=u dbname=db"));
        // sslmode=disable -> no TLS
        assert!(!conn_str_wants_tls(
            "postgres://user:pass@localhost/db?sslmode=disable"
        ));
        assert!(!conn_str_wants_tls("host=localhost sslmode=disable"));
        // any other sslmode -> TLS
        assert!(conn_str_wants_tls(
            "postgres://user:pass@localhost/db?sslmode=require"
        ));
        assert!(conn_str_wants_tls("host=localhost sslmode=prefer"));
        // sslmode mixed with other params -> TLS
        assert!(conn_str_wants_tls(
            "postgres://u:p@h/db?connect_timeout=10&sslmode=verify-full"
        ));
        // case-insensitive
        assert!(!conn_str_wants_tls("postgres://u:p@h/db?sslmode=DISABLE"));
    }

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
}
