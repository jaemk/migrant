use super::*;
/// Postgres database functions using shell commands and db drivers
use std;
use std::path::Path;

#[cfg(feature = "d-postgres")]
use postgres::{Client, NoTls};
use std::io::Read;

#[cfg(not(feature = "d-postgres"))]
mod m {
    use super::*;
    pub fn can_connect(cert: Option<&Path>, conn_str: &str) -> Result<bool> {
        unimplemented!("migrant_lib: must enable d-postgres feature");
    }
    pub fn migration_table_exists(cert: Option<&Path>, conn_str: &str) -> Result<bool> {
        unimplemented!("migrant_lib: must enable d-postgres feature");
    }
    pub fn migration_setup(cert: Option<&Path>, conn_str: &str) -> Result<bool> {
        unimplemented!("migrant_lib: must enable d-postgres feature");
    }
    pub fn select_migrations(cert: Option<&Path>, conn_str: &str) -> Result<Vec<String>> {
        unimplemented!("migrant_lib: must enable d-postgres feature");
    }
    pub fn insert_migration_tag(cert: Option<&Path>, conn_str: &str, tag: &str) -> Result<()> {
        unimplemented!("migrant_lib: must enable d-postgres feature");
    }
    pub fn remove_migration_tag(cert: Option<&Path>, conn_str: &str, tag: &str) -> Result<()> {
        unimplemented!("migrant_lib: must enable d-postgres feature");
    }
    pub fn run_migration(cert: Option<&Path>, conn_str: &str, filename: &Path) -> Result<()> {
        unimplemented!("migrant_lib: must enable d-postgres feature");
    }
    pub fn run_migration_str(cert: Option<&Path>, conn_str: &str, stmt: &str) -> Result<()> {
        unimplemented!("migrant_lib: must enable d-postgres feature");
    }
}

#[cfg(feature = "d-postgres")]
mod m {
    use super::*;
    macro_rules! make_connector {
        ($file:expr) => {{
            let cert = std::fs::read($file)
                .map_err(|e| format_err!(ErrorKind::Migration, "postgres cert file error {}", e))?;
            let cert = native_tls::Certificate::from_pem(&cert)
                .map_err(|e| format_err!(ErrorKind::Migration, "postgres cert load error {}", e))?;
            let connector = native_tls::TlsConnector::builder()
                .add_root_certificate(cert)
                .build()
                .map_err(|e| {
                    format_err!(ErrorKind::Migration, "postgres tls-connection error {}", e)
                })?;
            postgres_native_tls::MakeTlsConnector::new(connector)
        }};
    }

    /// Check connection
    pub fn can_connect(cert: Option<&Path>, conn_str: &str) -> Result<bool> {
        match cert {
            None => match Client::connect(conn_str, NoTls) {
                Ok(_) => Ok(true),
                Err(_) => Ok(false),
            },
            Some(cert) => match Client::connect(conn_str, make_connector!(cert)) {
                Ok(_) => Ok(true),
                Err(_) => Ok(false),
            },
        }
    }

    macro_rules! make_connection {
        ($cert:expr, $conn_str:expr) => {{
            match $cert {
                None => Client::connect($conn_str, NoTls),
                Some(cert) => Client::connect($conn_str, make_connector!(cert)),
            }
        }};
    }

    /// Check `__migrant_migrations` table exists
    pub fn migration_table_exists(cert: Option<&Path>, conn_str: &str) -> Result<bool> {
        let mut conn = make_connection!(cert, conn_str)
            .map_err(|e| format_err!(ErrorKind::Migration, "{}", e))?;

        let rows = conn
            .query(sql::PG_MIGRATION_TABLE_EXISTS, &[])
            .map_err(|e| format_err!(ErrorKind::Migration, "{}", e))?;
        let exists: bool = rows
            .get(0)
            .expect("expected a result from postgres when checking for migration table")
            .get(0);
        Ok(exists)
    }

    /// Create `__migrant_migrations` table
    pub fn migration_setup(cert: Option<&Path>, conn_str: &str) -> Result<bool> {
        if !migration_table_exists(cert, conn_str)? {
            let mut conn = make_connection!(cert, conn_str)
                .map_err(|e| format_err!(ErrorKind::Migration, "{}", e))?;
            conn.execute(sql::CREATE_TABLE, &[])
                .map_err(|e| format_err!(ErrorKind::Migration, "{}", e))?;
            return Ok(true);
        }
        Ok(false)
    }

    /// Select all migrations from `__migrant_migrations` table
    pub fn select_migrations(cert: Option<&Path>, conn_str: &str) -> Result<Vec<String>> {
        let mut conn = make_connection!(cert, conn_str)?;
        let rows = conn.query(sql::GET_MIGRATIONS, &[])?;
        Ok(rows.iter().map(|row| row.get(0)).collect())
    }

    /// Insert migration tag into `__migrant_migrations` table
    pub fn insert_migration_tag(cert: Option<&Path>, conn_str: &str, tag: &str) -> Result<()> {
        let mut conn = make_connection!(cert, conn_str)?;
        conn.execute(
            "insert into __migrant_migrations (tag) values ($1)",
            &[&tag],
        )?;
        Ok(())
    }

    /// Delete migration tag from `__migrant_migrations` table
    pub fn remove_migration_tag(cert: Option<&Path>, conn_str: &str, tag: &str) -> Result<()> {
        let mut conn = make_connection!(cert, conn_str)?;
        conn.execute("delete from __migrant_migrations where tag = $1", &[&tag])?;
        Ok(())
    }

    /// Apply migration to database
    pub fn run_migration(cert: Option<&Path>, conn_str: &str, filename: &Path) -> Result<()> {
        let mut file = std::fs::File::open(filename)?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;

        let mut conn = make_connection!(cert, conn_str)
            .map_err(|e| format_err!(ErrorKind::Migration, "{}", e))?;
        conn.batch_execute(&buf)
            .map_err(|e| format_err!(ErrorKind::Migration, "{}", e))?;
        Ok(())
    }

    pub fn run_migration_str(cert: Option<&Path>, conn_str: &str, stmt: &str) -> Result<()> {
        let mut conn = make_connection!(cert, conn_str)
            .map_err(|e| format_err!(ErrorKind::Migration, "{}", e))?;
        conn.batch_execute(stmt)
            .map_err(|e| format_err!(ErrorKind::Migration, "{}", e))?;
        Ok(())
    }
}

pub use self::m::*;

#[cfg(feature = "d-postgres")]
#[cfg(test)]
mod test {
    use super::*;
    use std;
    macro_rules! _try {
        ($exp:expr) => {
            match $exp {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Caught: {}", e);
                    panic!("{}", e)
                }
            }
        };
    }

    #[test]
    fn postgres() {
        let conn_str = std::env::var("POSTGRES_TEST_CONN_STR")
            .expect("POSTGRES_TEST_CONN_STR env variable required");

        // no table before setup
        assert!(can_connect(None, &conn_str).is_ok());
        let is_setup = _try!(migration_table_exists(None, &conn_str));
        assert!(!is_setup, "Assert migration table does not exist");

        // setup migration table
        let was_setup = _try!(migration_setup(None, &conn_str));
        assert!(
            was_setup,
            "Assert `migration_setup` initializes migration table"
        );
        let was_setup = _try!(migration_setup(None, &conn_str));
        assert!(!was_setup, "Assert `migration_setup` is idempotent");

        // table exists after setup
        let is_setup = _try!(migration_table_exists(None, &conn_str));
        assert!(is_setup, "Assert migration table exists");

        // insert some tags
        _try!(insert_migration_tag(None, &conn_str, "initial"));
        _try!(insert_migration_tag(None, &conn_str, "alter1"));
        _try!(insert_migration_tag(None, &conn_str, "alter2"));

        // get applied
        let migs = _try!(select_migrations(None, &conn_str));
        assert_eq!(3, migs.len(), "Assert 3 migrations applied");

        // remove some tags
        _try!(remove_migration_tag(None, &conn_str, "alter2"));
        let migs = _try!(select_migrations(None, &conn_str));
        assert_eq!(2, migs.len(), "Assert 2 migrations applied");

        _try!(remove_migration_tag(None, &conn_str, "alter1"));
        _try!(remove_migration_tag(None, &conn_str, "initial"));
        let migs = _try!(select_migrations(None, &conn_str));
        assert_eq!(0, migs.len(), "Assert all migrations removed");
    }
}
