use super::*;
use std::fs;
use std::path::Path;

#[cfg(feature = "d-sqlite")]
use rusqlite::Connection;
use std::io::Read;

#[cfg(not(feature = "d-sqlite"))]
mod m {
    use super::*;
    pub fn create_file_if_missing(path: &Path) -> Result<bool> {
        unimplemented!("migrant_lib: must enable d-sqlite feature");
    }
    pub fn migration_table_exists(db_path: &str) -> Result<bool> {
        unimplemented!("migrant_lib: must enable d-sqlite feature");
    }
    pub fn migration_setup(db_path: &Path) -> Result<bool> {
        unimplemented!("migrant_lib: must enable d-sqlite feature");
    }
    pub fn select_migrations(db_path: &str) -> Result<Vec<String>> {
        unimplemented!("migrant_lib: must enable d-sqlite feature");
    }
    pub fn insert_migration_tag(db_path: &str, tag: &str) -> Result<()> {
        unimplemented!("migrant_lib: must enable d-sqlite feature");
    }
    pub fn remove_migration_tag(db_path: &str, tag: &str) -> Result<()> {
        unimplemented!("migrant_lib: must enable d-sqlite feature");
    }
    pub fn run_migration(db_path: &Path, filename: &Path) -> Result<()> {
        unimplemented!("migrant_lib: must enable d-sqlite feature");
    }
    pub fn run_migration_str(db_path: &Path, stmt: &str) -> Result<()> {
        unimplemented!("migrant_lib: must enable d-sqlite feature");
    }
}

#[cfg(feature = "d-sqlite")]
mod m {
    use super::*;
    /// Check database exists / create it
    /// Create a file if it doesn't exist, returning true if the file was created
    pub fn create_file_if_missing(path: &Path) -> Result<bool> {
        if path == Path::new(":memory:") || path.exists() {
            Ok(false)
        } else {
            let db_dir = path.parent().ok_or_else(|| {
                format_err!(
                    ErrorKind::PathError,
                    "Unable to determine parent path: {:?}",
                    path
                )
            })?;
            fs::create_dir_all(db_dir)
                .chain_err(|| format!("Failed creating database directory: {:?}", db_dir))?;
            fs::File::create(path)
                .chain_err(|| format!("Failed creating database file: {:?}", path))?;
            Ok(true)
        }
    }

    /// Check `__migrant_migrations` table exists
    pub fn migration_table_exists(db_path: &str) -> Result<bool> {
        let conn = Connection::open(db_path)?;
        let exists: bool =
            conn.query_row(sql::SQLITE_MIGRATION_TABLE_EXISTS, [], |row| row.get(0))?;
        Ok(exists)
    }

    /// Create `__migrant_migrations` table
    pub fn migration_setup(db_path: &Path) -> Result<bool> {
        let db_path = db_path.to_str().unwrap();
        if !migration_table_exists(db_path)? {
            let conn = Connection::open(db_path)?;
            conn.execute(sql::CREATE_TABLE, [])?;
            return Ok(true);
        }
        Ok(false)
    }

    /// Select all migrations from `__migrant_migrations` table
    pub fn select_migrations(db_path: &str) -> Result<Vec<String>> {
        let conn = Connection::open(db_path)?;
        let mut stmt = conn.prepare(sql::GET_MIGRATIONS)?;
        let mut rows = stmt.query([])?;
        let mut migs = vec![];
        while let Some(row) = rows.next()? {
            migs.push(row.get(0)?);
        }
        Ok(migs)
    }

    /// Insert tag into `__migrant_migrations` table
    pub fn insert_migration_tag(db_path: &str, tag: &str) -> Result<()> {
        let conn = Connection::open(db_path)?;
        conn.execute(
            "insert into __migrant_migrations (tag) values ($1)",
            &[&tag],
        )?;
        Ok(())
    }

    /// Remove tag from `__migrant_migrations` table
    pub fn remove_migration_tag(db_path: &str, tag: &str) -> Result<()> {
        let conn = Connection::open(db_path)?;
        conn.execute("delete from __migrant_migrations where tag = $1", &[&tag])?;
        Ok(())
    }

    /// Apply migration file to database
    pub fn run_migration(db_path: &Path, filename: &Path) -> Result<()> {
        let mut file = fs::File::open(filename)?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        if buf.is_empty() {
            return Ok(());
        }

        let conn =
            Connection::open(db_path).map_err(|e| format_err!(ErrorKind::Migration, "{}", e))?;
        conn.execute_batch(&buf)
            .map_err(|e| format_err!(ErrorKind::Migration, "{}", e))?;
        Ok(())
    }

    pub fn run_migration_str(db_path: &Path, stmt: &str) -> Result<()> {
        if stmt.is_empty() {
            return Ok(());
        }

        let conn =
            Connection::open(db_path).map_err(|e| format_err!(ErrorKind::Migration, "{}", e))?;
        conn.execute_batch(stmt)
            .map_err(|e| format_err!(ErrorKind::Migration, "{}", e))?;
        Ok(())
    }
}

pub use self::m::*;

#[cfg(feature = "d-sqlite")]
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
    fn sqlite() {
        let conn_str =
            std::env::var("SQLITE_TEST_CONN_STR").expect("SQLITE_TEST_CONN_STR env var required");
        let path = std::path::Path::new(&conn_str);

        // no table before setup
        let is_setup = _try!(migration_table_exists(&conn_str));
        assert!(!is_setup, "Assert migration table does not exist");

        // setup migration table
        let was_setup = _try!(migration_setup(path));
        assert!(
            was_setup,
            "Assert `migration_setup` initializes migration table"
        );
        let was_setup = _try!(migration_setup(path));
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
