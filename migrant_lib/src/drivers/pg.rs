/*!
Postgres driver
*/
use std::path::Path;

use postgres::{Client, NoTls};

use super::sql;
use crate::errors::*;
use crate::macros::err;

/// Session-level advisory lock key that serializes concurrent migration runs.
///
/// The value is arbitrary but must be identical across every process using
/// this library so they contend for the same lock. `pg_advisory_lock` takes a
/// single `bigint`; this constant is stable and namespaced to migrant.
const ADVISORY_LOCK_KEY: i64 = 30_796_665_483_397_364;

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

    pub(crate) fn begin(&mut self) -> Result<()> {
        self.client
            .batch_execute("begin")
            .map_err(|e| err!(Migration, "{}", e))
    }

    pub(crate) fn commit(&mut self) -> Result<()> {
        self.client
            .batch_execute("commit")
            .map_err(|e| err!(Migration, "{}", e))
    }

    pub(crate) fn rollback(&mut self) -> Result<()> {
        self.client
            .batch_execute("rollback")
            .map_err(|e| err!(Migration, "{}", e))
    }

    /// Take the session-level advisory lock, blocking until it is available.
    /// Postgres releases it automatically if this connection (session) drops.
    pub(crate) fn acquire_lock(&mut self) -> Result<()> {
        self.client
            .execute("select pg_advisory_lock($1)", &[&ADVISORY_LOCK_KEY])
            .map_err(|e| err!(Migration, "{}", e))?;
        Ok(())
    }

    pub(crate) fn release_lock(&mut self) -> Result<()> {
        self.client
            .execute("select pg_advisory_unlock($1)", &[&ADVISORY_LOCK_KEY])
            .map_err(|e| err!(Migration, "{}", e))?;
        Ok(())
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

    /// Advisory-lock behavior. Both scenarios share the one fixed lock key, so
    /// they run as a single test rather than racing each other for it under
    /// cargo's parallel runner. Requires a running postgres instance
    /// (`POSTGRES_TEST_CONN_STR`).
    #[test]
    fn advisory_lock() {
        let conn_str = match std::env::var("POSTGRES_TEST_CONN_STR") {
            Ok(s) => s,
            Err(_) => {
                eprintln!("POSTGRES_TEST_CONN_STR not set, skipping");
                return;
            }
        };
        lock_is_exclusive(&conn_str);
        lock_survives_in_transaction_error(&conn_str);
    }

    /// While one session holds the lock, another cannot, and it becomes
    /// available again once released.
    fn lock_is_exclusive(conn_str: &str) {
        let mut holder = PgConn::connect(conn_str, None).unwrap();
        let mut other = PgConn::connect(conn_str, None).unwrap();

        // `pg_try_advisory_lock` returns immediately with whether it got the lock.
        let try_lock = |c: &mut PgConn| -> bool {
            c.client
                .query_one("select pg_try_advisory_lock($1)", &[&ADVISORY_LOCK_KEY])
                .unwrap()
                .get(0)
        };

        holder.acquire_lock().unwrap();
        assert!(
            !try_lock(&mut other),
            "second session must not acquire a held lock"
        );
        holder.release_lock().unwrap();
        assert!(try_lock(&mut other), "lock must be available once released");
        // release the try-lock we just took so we don't leak it on the session
        other
            .client
            .execute("select pg_advisory_unlock($1)", &[&ADVISORY_LOCK_KEY])
            .unwrap();
    }

    /// After an error inside a transaction, recovering the connection in place
    /// with `rollback` (as `Config::with_conn` does) keeps the session alive, so
    /// the advisory lock it holds survives the error and the connection stays
    /// usable. This is what lets a `force`d migration run keep holding the lock
    /// past a failed migration.
    fn lock_survives_in_transaction_error(conn_str: &str) {
        let mut holder = PgConn::connect(conn_str, None).unwrap();
        let mut other = PgConn::connect(conn_str, None).unwrap();

        holder.acquire_lock().unwrap();
        // Provoke an error inside an explicit transaction: postgres leaves the
        // connection in an aborted-transaction state.
        holder.begin().unwrap();
        assert!(holder
            .execute_batch("select * from does_not_exist")
            .is_err());
        // Recover in place, exactly as `with_conn` does on a server error.
        holder.rollback().unwrap();

        // The session survived, so it still holds the lock and another session
        // cannot take it (`not pg_try_advisory_lock` is true when it is held).
        let still_held: bool = other
            .client
            .query_one("select not pg_try_advisory_lock($1)", &[&ADVISORY_LOCK_KEY])
            .unwrap()
            .get(0);
        assert!(
            still_held,
            "advisory lock must survive an in-transaction error"
        );
        // The recovered connection is usable again.
        let one: i32 = holder.client.query_one("select 1", &[]).unwrap().get(0);
        assert_eq!(1, one);

        holder.release_lock().unwrap();
        // Drop any advisory locks `other` may have taken so none leak on its session.
        other
            .client
            .execute("select pg_advisory_unlock_all()", &[])
            .unwrap();
    }
}
