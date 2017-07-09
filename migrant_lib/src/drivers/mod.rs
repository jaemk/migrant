use std;
use std::fs;
use std::process::Command;
use std::path::Path;

#[cfg(feature="postgresql")]
use std::io::Read;

use super::errors::*;


mod sql {
    pub static CREATE_TABLE: &'static str = "create table __migrant_migrations(tag text unique);";
    pub static GET_MIGRATIONS: &'static str = "select tag from __migrant_migrations;";

    pub static SQLITE_MIGRATION_TABLE_EXISTS: &'static str = "select exists(select 1 from sqlite_master where type = 'table' and name = '__migrant_migrations');";
    pub static PG_MIGRATION_TABLE_EXISTS: &'static str = "select exists(select 1 from pg_tables where tablename = '__migrant_migrations');";

    // Some of these queries need to do unsafe search/replace of `__VAL__` -> tag
    // All tags are validated when created and again when loaded from the database migration table,
    // limiting chars to `[a-z0-9-]` and the full pattern to `[0-9]{14}_[a-z0-9-]+` so even if malicious
    // tags find their way into the database, tag validators should raise errors and point them out
    #[cfg(not(feature="sqlite"))]
    pub use self::q_sqlite::*;
    #[cfg(not(feature="sqlite"))]
    mod q_sqlite {
        pub static SQLITE_ADD_MIGRATION: &'static str = "insert into __migrant_migrations (tag) values ('__VAL__');";
        pub static SQLITE_DELETE_MIGRATION: &'static str = "delete from __migrant_migrations where tag = '__VAL__';";
    }

    #[cfg(not(feature="postgresql"))]
    pub use self::q_postgres::*;
    #[cfg(not(feature="postgresql"))]
    mod q_postgres {
        pub static PG_ADD_MIGRATION: &'static str = "prepare stmt as insert into __migrant_migrations (tag) values ($1); execute stmt('__VAL__'); deallocate stmt;";
        pub static PG_DELETE_MIGRATION: &'static str = "prepare stmt as delete from __migrant_migrations where tag = $1; execute stmt('__VAL__'); deallocate stmt;";
    }
}


/// Postgres database functions using shell commands and db drivers
pub mod pg {
    use super::*;

    // --
    // Check connection
    // --
    #[cfg(not(feature="postgresql"))]
    pub fn can_connect(connect_string: &str) -> Result<bool> {
        let out = Command::new("psql")
                        .arg(connect_string)
                        .arg("-c")
                        .arg("")
                        .output()
                        .map_err(Error::IoProc)?;
        Ok(out.status.success())
    }

    #[cfg(feature="postgresql")]
    pub fn can_connect(conn_str: &str) -> Result<bool> {
        use postgres::{Connection, TlsMode};

        match Connection::connect(conn_str, TlsMode::None) {
            Ok(_)   => Ok(true),
            Err(_)  => Ok(false)
        }
    }


    // --
    // Check `__migrant_migrations` table exists
    // --
    #[cfg(not(feature="postgresql"))]
    pub fn migration_table_exists(conn_str: &str) -> Result<bool> {
        let exists = Command::new("psql")
                        .arg(conn_str)
                        .arg("-t")      // no headers or footer
                        .arg("-A")      // un-aligned output
                        .arg("-F,")     // comma separator
                        .arg("-c")
                        .arg(sql::PG_MIGRATION_TABLE_EXISTS)
                        .output()
                        .map_err(Error::IoProc)?;
        if !exists.status.success() {
            let stderr = std::str::from_utf8(&exists.stderr).unwrap();
            bail!(Migration <- "Error executing statement: {}", stderr);
        }
        let stdout = std::str::from_utf8(&exists.stdout).unwrap();
        Ok(stdout.trim() == "t")
    }

    #[cfg(feature="postgresql")]
    pub fn migration_table_exists(conn_str: &str) -> Result<bool> {
        use postgres::{Connection, TlsMode};

        let conn = Connection::connect(conn_str, TlsMode::None)
            .map_err(|e| format_err!(Error::Migration, "{}", e))?;
        let rows = conn.query(sql::PG_MIGRATION_TABLE_EXISTS, &[])
            .map_err(|e| format_err!(Error::Migration, "{}", e))?;
        let exists: bool = rows.iter().next().unwrap().get(0);
        Ok(exists)
    }


    // --
    // Create `__migrant_migrations` table
    // --
    #[cfg(not(feature="postgresql"))]
    pub fn migration_setup(conn_str: &str) -> Result<bool> {
        if !migration_table_exists(conn_str)? {
            let out = Command::new("psql")
                            .arg(conn_str)
                            .arg("-t")
                            .arg("-A")
                            .arg("-F,")
                            .arg("-c")
                            .arg(sql::CREATE_TABLE)
                            .output()
                            .map_err(Error::IoProc)?;
            if !out.status.success() {
                let stderr = std::str::from_utf8(&out.stderr).unwrap();
                bail!(Migration <- "Error executing statement: {}", stderr);
            }
            return Ok(true)
        }
        Ok(false)
    }

    #[cfg(feature="postgresql")]
    pub fn migration_setup(conn_str: &str) -> Result<bool> {
        use postgres::{Connection, TlsMode};

        if !migration_table_exists(conn_str)? {
            let conn = Connection::connect(conn_str, TlsMode::None)
                .map_err(|e| format_err!(Error::Migration, "{}", e))?;
            conn.execute(sql::CREATE_TABLE, &[])
                .map_err(|e| format_err!(Error::Migration, "{}", e))?;
            return Ok(true)
        }
        Ok(false)
    }


    // --
    // Select all migrations from `__migrant_migrations` table
    // --
    #[cfg(not(feature="postgresql"))]
    pub fn select_migrations(conn_str: &str) -> Result<Vec<String>> {
        let migs = Command::new("psql")
                        .arg(conn_str)
                        .arg("-t")      // no headers or footer
                        .arg("-A")      // un-aligned output
                        .arg("-F,")     // comma separator
                        .arg("-c")
                        .arg(sql::GET_MIGRATIONS)
                        .output()
                        .map_err(Error::IoProc)?;
        if !migs.status.success() {
            let stderr = std::str::from_utf8(&migs.stderr).unwrap();
            bail!(Migration <- "Error executing statement: {}", stderr);
        }
        let stdout = std::str::from_utf8(&migs.stdout).unwrap();
        Ok(stdout.trim().lines().map(String::from).collect())
    }

    #[cfg(feature="postgresql")]
    pub fn select_migrations(conn_str: &str) -> Result<Vec<String>> {
        use postgres::{Connection, TlsMode};

        let conn = Connection::connect(conn_str, TlsMode::None)?;
        let rows = conn.query(sql::GET_MIGRATIONS, &[])?;
        Ok(rows.iter().map(|row| row.get(0)).collect())
    }


    // --
    // Insert migration tag into `__migrant_migrations` table
    // --
    #[cfg(not(feature="postgresql"))]
    pub fn insert_migration_tag(conn_str: &str, tag: &str) -> Result<()> {
        let insert = Command::new("psql")
                        .arg(conn_str)
                        .arg("-t")      // no headers or footer
                        .arg("-A")      // un-aligned output
                        .arg("-F,")     // comma separator
                        .arg("-c")
                        .arg(sql::PG_ADD_MIGRATION.replace("__VAL__", tag))
                        .output()
                        .map_err(Error::IoProc)?;
        if !insert.status.success() {
            let stderr = std::str::from_utf8(&insert.stderr).unwrap();
            bail!(Migration <- "Error executing statement: {}", stderr);
        }
        Ok(())
    }

    #[cfg(feature="postgresql")]
    pub fn insert_migration_tag(conn_str: &str, tag: &str) -> Result<()> {
        use postgres::{Connection, TlsMode};

        let conn = Connection::connect(conn_str, TlsMode::None)?;
        conn.execute("insert into __migrant_migrations (tag) values ($1)", &[&tag])?;
        Ok(())
    }


    // --
    // Delete migration tag from `__migrant_migrations` table
    // --
    #[cfg(not(feature="postgresql"))]
    pub fn remove_migration_tag(conn_str: &str, tag: &str) -> Result<()> {
        let insert = Command::new("psql")
                        .arg(conn_str)
                        .arg("-t")      // no headers or footer
                        .arg("-A")      // un-aligned output
                        .arg("-F,")     // comma separator
                        .arg("-c")
                        .arg(sql::PG_DELETE_MIGRATION.replace("__VAL__", tag))
                        .output()
                        .map_err(Error::IoProc)?;
        if !insert.status.success() {
            let stderr = std::str::from_utf8(&insert.stderr).unwrap();
            bail!(Migration <- "Error executing statement: {}", stderr);
        }
        Ok(())
    }

    #[cfg(feature="postgresql")]
    pub fn remove_migration_tag(conn_str: &str, tag: &str) -> Result<()> {
        use postgres::{Connection, TlsMode};

        let conn = Connection::connect(conn_str, TlsMode::None)?;
        conn.execute("delete from __migrant_migrations where tag = $1", &[&tag])?;
        Ok(())
    }


    // --
    // Apply migration to database
    // --
    #[cfg(feature="postgresql")]
    pub fn run_migration(conn_str: &str, filename: &str) -> Result<()> {
        use postgres::{Connection, TlsMode};

        let mut file = fs::File::open(filename)
            .map_err(Error::IoOpen)?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)
            .map_err(Error::IoRead)?;

        let conn = Connection::connect(conn_str, TlsMode::None)
            .map_err(|e| format_err!(Error::Migration, "{}", e))?;
        conn.execute(&buf, &[])
            .map_err(|e| format_err!(Error::Migration, "{}", e))?;
        Ok(())
    }

    /// Fall back to running the migration using the postgres cli
    #[cfg(not(feature="postgresql"))]
    pub fn run_migration(conn_str: &str, filename: &str) -> Result<()> {
        Command::new("psql")
                .arg(&conn_str)
                .arg("-f").arg(filename)
                .output()
                .map_err(Error::IoProc)?;
        Ok(())
    }
}


pub mod sqlite {
    use super::*;


    // --
    // Check database exists / create it
    // --
    /// Create a file if it doesn't exist, returning true if the file was created
    pub fn create_file_if_missing(path: &Path) -> Result<bool> {
        if path.exists() {
            Ok(false)
        } else {
            let db_dir = path.parent().unwrap();
            fs::create_dir(db_dir).map_err(Error::IoCreate)?;
            fs::File::create(path).map_err(Error::IoCreate)?;
            Ok(true)
        }
    }


    // --
    // Check `__migrant_migrations` table exists
    // --
    #[cfg(not(feature="sqlite"))]
    pub fn migration_table_exists(db_path: &str) -> Result<bool> {
        let exists = Command::new("sqlite3")
                        .arg(&db_path)
                        .arg("-csv")
                        .arg(sql::SQLITE_MIGRATION_TABLE_EXISTS)
                        .output()
                        .map_err(Error::IoProc)?;
        if !exists.status.success() {
            let stderr = std::str::from_utf8(&exists.stderr).unwrap();
            bail!(Migration <- "Error executing statement: {}", stderr);
        }
        let stdout = std::str::from_utf8(&exists.stdout).unwrap();
        Ok(stdout.trim() == "1")
    }

    #[cfg(feature="sqlite")]
    pub fn migration_table_exists(db_path: &str) -> Result<bool> {
        use rusqlite::Connection;

        let conn = Connection::open(db_path)?;
        let exists: bool = conn.query_row(sql::SQLITE_MIGRATION_TABLE_EXISTS, &[], |row| row.get(0))?;
        Ok(exists)
    }


    // --
    // Create `__migrant_migrations` table
    // --
    #[cfg(not(feature="sqlite"))]
    pub fn migration_setup(db_path: &Path) -> Result<bool> {
        let db_path = db_path.as_os_str().to_str().unwrap();
        if !migration_table_exists(db_path)? {
            let out = Command::new("sqlite3")
                            .arg(&db_path)
                            .arg("-csv")
                            .arg(sql::CREATE_TABLE)
                            .output()
                            .map_err(Error::IoProc)?;
            if !out.status.success() {
                let stderr = std::str::from_utf8(&out.stderr).unwrap();
                bail!(Migration <- "Error executing statement: {}", stderr);
            }
            return Ok(true)
        }
        Ok(false)
    }

    #[cfg(feature="sqlite")]
    pub fn migration_setup(db_path: &Path) -> Result<bool> {
        use rusqlite::Connection;

        let db_path = db_path.to_str().unwrap();
        if !migration_table_exists(db_path)? {
            let conn = Connection::open(db_path)?;
            conn.execute(sql::CREATE_TABLE, &[])?;
            return Ok(true)
        }
        Ok(false)
    }


    // --
    // Select all migrations from `__migrant_migrations` table
    // --
    #[cfg(not(feature="sqlite"))]
    pub fn select_migrations(db_path: &str) -> Result<Vec<String>> {
        let migs = Command::new("sqlite3")
                        .arg(&db_path)
                        .arg("-csv")
                        .arg(sql::GET_MIGRATIONS)
                        .output()
                        .map_err(Error::IoProc)?;
        if !migs.status.success() {
            let stderr = std::str::from_utf8(&migs.stderr).unwrap();
            bail!(Migration <- "Error executing statement: {}", stderr);
        }
        let stdout = std::str::from_utf8(&migs.stdout).unwrap();
        Ok(stdout.trim().lines().map(String::from).collect::<Vec<_>>())
    }

    #[cfg(feature="sqlite")]
    pub fn select_migrations(db_path: &str) -> Result<Vec<String>> {
        use rusqlite::Connection;

        let conn = Connection::open(db_path)?;
        let mut stmt = conn.prepare(sql::GET_MIGRATIONS)?;
        let mut rows = stmt.query(&[])?;
        let mut migs = vec![];
        while let Some(row) = rows.next() {
            let row = row?;
            migs.push(row.get(0));
        }
        Ok(migs)
    }


    // --
    // Insert tag into `__migrant_migrations` table
    // --
    #[cfg(not(feature="sqlite"))]
    pub fn insert_migration_tag(db_path: &str, tag: &str) -> Result<()> {
        let stmt = sql::SQLITE_ADD_MIGRATION.replace("__VAL__", tag);
        println!("stmt: {}", stmt);
        let insert = Command::new("sqlite3")
                        .arg(&db_path)
                        .arg("-csv")
                        .arg(sql::SQLITE_ADD_MIGRATION.replace("__VAL__", tag))
                        .output()
                        .map_err(Error::IoProc)?;
        if !insert.status.success() {
            let stderr = std::str::from_utf8(&insert.stderr).unwrap();
            bail!(Migration <- "Error executing statement: {}", stderr);
        }
        Ok(())
    }

    #[cfg(feature="sqlite")]
    pub fn insert_migration_tag(db_path: &str, tag: &str) -> Result<()> {
        use rusqlite::Connection;

        let conn = Connection::open(db_path)?;
        conn.execute("insert into __migrant_migrations (tag) values ($1)", &[&tag])?;
        Ok(())
    }


    // --
    // Remove tag from `__migrant_migrations` table
    // --
    #[cfg(not(feature="sqlite"))]
    pub fn remove_migration_tag(db_path: &str, tag: &str) -> Result<()> {
        let exists = Command::new("sqlite3")
                        .arg(&db_path)
                        .arg("-csv")
                        .arg(sql::SQLITE_DELETE_MIGRATION.replace("__VAL__", tag))
                        .output()
                        .map_err(Error::IoProc)?;
        if !exists.status.success() {
            let stderr = std::str::from_utf8(&exists.stderr).unwrap();
            bail!(Migration <- "Error executing statement: {}", stderr);
        }
        Ok(())
    }

    #[cfg(feature="sqlite")]
    pub fn remove_migration_tag(db_path: &str, tag: &str) -> Result<()> {
        use rusqlite::Connection;

        let conn = Connection::open(db_path)?;
        conn.execute("delete from __migrant_migrations where tag = $1", &[&tag])?;
        Ok(())
    }


    // --
    // Apply migration file to database
    // --
    /// Fall back to running the migration using the sqlite cli
    #[cfg(not(feature="sqlite"))]
    pub fn run_migration(db_path: &Path, filename: &str) -> Result<()> {
        Command::new("sqlite3")
                .arg(db_path.to_str().unwrap())
                .arg(&format!(".read {}", filename))
                .output()
                .map_err(Error::IoProc)?;
        Ok(())
    }

    #[cfg(feature="sqlite")]
    pub fn run_migration(db_path: &Path, filename: &str) -> Result<()> {
        use rusqlite::Connection;

        let mut file = fs::File::open(filename)
            .map_err(Error::IoOpen)?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)
            .map_err(Error::IoRead)?;
        if buf.is_empty() { return Ok(()); }

        let conn = Connection::open(db_path)
            .map_err(|e| format_err!(Error::Migration, "{}", e))?;
        conn.execute(&buf, &[])
            .map_err(|e| format_err!(Error::Migration, "{}", e))?;
        Ok(())
    }
}

