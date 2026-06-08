use super::*;
/// MySQL database functions using shell commands and db drivers
use std;
use std::path::Path;

use std::io::Read;

#[cfg(feature = "d-mysql")]
use ::mysql::{prelude::*, Conn, Opts};

#[cfg(not(feature = "d-mysql"))]
mod m {
    use super::*;
    pub fn can_connect(conn_str: &str) -> Result<bool> {
        unimplemented!("migrant_lib: must enable d-mysql feature");
    }
    pub fn migration_table_exists(conn_str: &str) -> Result<bool> {
        unimplemented!("migrant_lib: must enable d-mysql feature");
    }
    pub fn migration_setup(conn_str: &str) -> Result<bool> {
        unimplemented!("migrant_lib: must enable d-mysql feature");
    }
    pub fn select_migrations(conn_str: &str) -> Result<Vec<String>> {
        unimplemented!("migrant_lib: must enable d-mysql feature");
    }
    pub fn insert_migration_tag(conn_str: &str, tag: &str) -> Result<()> {
        unimplemented!("migrant_lib: must enable d-mysql feature");
    }
    pub fn remove_migration_tag(conn_str: &str, tag: &str) -> Result<()> {
        unimplemented!("migrant_lib: must enable d-mysql feature");
    }
    pub fn run_migration(conn_str: &str, filename: &Path) -> Result<()> {
        unimplemented!("migrant_lib: must enable d-mysql feature");
    }
    pub fn run_migration_str(conn_str: &str, stmt: &str) -> Result<()> {
        unimplemented!("migrant_lib: must enable d-mysql feature");
    }
}

#[cfg(feature = "d-mysql")]
mod m {
    use super::*;
    /// Check connection
    pub fn can_connect(conn_str: &str) -> Result<bool> {
        let conn_opts = Opts::from_url(conn_str)
            .chain_err(|| "Error parsing mysql connection string".to_string())?;
        Conn::new(conn_opts).chain_err(|| {
            format!(
                "Unable to connect to mysql database with conn str: {:?}",
                conn_str
            )
        })?;
        Ok(true)
    }

    /// Check `__migrant_migrations` table exists
    pub fn migration_table_exists(conn_str: &str) -> Result<bool> {
        let conn_str = Opts::from_url(conn_str)
            .chain_err(|| "Error parsing mysql connection string".to_string())?;
        let mut conn = Conn::new(conn_str).chain_err(|| "Connection Error")?;
        let rows: Vec<u32> = conn.query(sql::MYSQL_MIGRATION_TABLE_EXISTS)?;
        assert_eq!(
            rows.len(),
            1,
            "Migration table check: Expected 1 returned row"
        );
        Ok(rows[0] == 1)
    }

    /// Create `__migrant_migrations` table
    pub fn migration_setup(conn_str: &str) -> Result<bool> {
        if !migration_table_exists(conn_str)? {
            let conn_str = Opts::from_url(conn_str)
                .chain_err(|| "Error parsing mysql connection string".to_string())?;
            let mut conn = Conn::new(conn_str).chain_err(|| "Connection Error")?;
            conn.query_drop(sql::MYSQL_CREATE_TABLE)
                .chain_err(|| "Error setting up migration table")?;
            return Ok(true);
        }
        Ok(false)
    }

    /// Select all migrations from `__migrant_migrations` table
    pub fn select_migrations(conn_str: &str) -> Result<Vec<String>> {
        let conn_str = Opts::from_url(conn_str)
            .chain_err(|| "Error parsing mysql connection string".to_string())?;
        let mut conn = Conn::new(conn_str).chain_err(|| "Connection Error")?;
        Ok(conn.query(sql::GET_MIGRATIONS)?)
    }

    /// Insert migration tag into `__migrant_migrations` table
    pub fn insert_migration_tag(conn_str: &str, tag: &str) -> Result<()> {
        let conn_str = Opts::from_url(conn_str)
            .chain_err(|| "Error parsing mysql connection string".to_string())?;
        let mut conn = Conn::new(conn_str).chain_err(|| "Connection Error")?;
        conn.exec_drop("insert into __migrant_migrations (tag) values (?)", (tag,))?;
        Ok(())
    }

    /// Delete migration tag from `__migrant_migrations` table
    pub fn remove_migration_tag(conn_str: &str, tag: &str) -> Result<()> {
        let conn_str = Opts::from_url(conn_str)
            .chain_err(|| "Error parsing mysql connection string".to_string())?;
        let mut conn = Conn::new(conn_str).chain_err(|| "Connection Error")?;
        conn.exec_drop("delete from __migrant_migrations where tag = ?", (tag,))?;
        Ok(())
    }

    /// Apply migration to database
    pub fn run_migration(conn_str: &str, filename: &Path) -> Result<()> {
        let mut file = std::fs::File::open(filename)?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;

        let conn_str = Opts::from_url(conn_str)
            .chain_err(|| "Error parsing mysql connection string".to_string())?;
        let mut conn = Conn::new(conn_str).chain_err(|| "Connection Error")?;
        conn.query_drop(&buf)
            .map_err(|e| format_err!(ErrorKind::Migration, "{}", e))?;
        Ok(())
    }

    pub fn run_migration_str(conn_str: &str, stmt: &str) -> Result<()> {
        let conn_str = Opts::from_url(conn_str)
            .chain_err(|| "Error parsing mysql connection string".to_string())?;
        let mut conn = Conn::new(conn_str).chain_err(|| "Connection Error")?;
        conn.query_drop(stmt)
            .map_err(|e| format_err!(ErrorKind::Migration, "{}", e))?;
        Ok(())
    }
}

pub use self::m::*;

#[cfg(feature = "d-mysql")]
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
    fn mysql() {
        let conn_str = std::env::var("MYSQL_TEST_CONN_STR")
            .expect("MYSQL_TEST_CONN_STR env variable required");

        // no table before setup
        can_connect(&conn_str).unwrap();
        let is_setup = _try!(migration_table_exists(&conn_str));
        assert!(!is_setup, "Assert migration table does not exist");

        // setup migration table
        let was_setup = _try!(migration_setup(&conn_str));
        assert!(
            was_setup,
            "Assert `migration_setup` initializes migration table"
        );
        let was_setup = _try!(migration_setup(&conn_str));
        assert!(!was_setup, "Assert `migration_setup` is idempotent");

        // table exists after setup
        let is_setup = _try!(migration_table_exists(&conn_str));
        assert!(is_setup, "Assert migration table exists");

        // insert some tags
        _try!(insert_migration_tag(&conn_str, "initial"));
        _try!(insert_migration_tag(&conn_str, "alter1"));
        _try!(insert_migration_tag(&conn_str, "alter2"));

        // get applied
        let migs = _try!(select_migrations(&conn_str));
        assert_eq!(3, migs.len(), "Assert 3 migrations applied");

        // remove some tags
        _try!(remove_migration_tag(&conn_str, "alter2"));
        let migs = _try!(select_migrations(&conn_str));
        assert_eq!(2, migs.len(), "Assert 2 migrations applied");

        _try!(remove_migration_tag(&conn_str, "alter1"));
        _try!(remove_migration_tag(&conn_str, "initial"));
        let migs = _try!(select_migrations(&conn_str));
        assert_eq!(0, migs.len(), "Assert all migrations removed");
    }
}
