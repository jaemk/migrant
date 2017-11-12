/// Postgres database functions using shell commands and db drivers
use std;
use super::*;

#[cfg(feature="postgresql")]
use std::io::Read;
#[cfg(feature="postgresql")]
use postgres::{Connection, TlsMode};

#[cfg(not(feature="postgresql"))]
use std::process::Command;


#[cfg(not(feature="postgresql"))]
fn psql_cmd(conn_str: &str, cmd: &str) -> Result<String> {
    let out = Command::new("psql")
                    .arg(conn_str)
                    .arg("-t")      // no headers or footer
                    .arg("-A")      // un-aligned output
                    .arg("-F,")     // comma separator
                    .arg("-c")
                    .arg(cmd)
                    .output()?;
    if !out.status.success() {
        let stderr = std::str::from_utf8(&out.stderr)?;
        bail_fmt!(ErrorKind::Migration, "Error executing statement, stderr: `{}`", stderr);
    }
    let stdout = String::from_utf8(out.stdout)?;
    Ok(stdout)
}


// --
// Check connection
// --
#[cfg(not(feature="postgresql"))]
pub fn can_connect(conn_str: &str) -> Result<bool> {
    let out = Command::new("psql")
                    .arg(conn_str)
                    .arg("-c")
                    .arg("")
                    .output()?;
    Ok(out.status.success())
}

#[cfg(feature="postgresql")]
pub fn can_connect(conn_str: &str) -> Result<bool> {
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
    let stdout = psql_cmd(conn_str, sql::PG_MIGRATION_TABLE_EXISTS)?;
    Ok(stdout.trim() == "t")
}

#[cfg(feature="postgresql")]
pub fn migration_table_exists(conn_str: &str) -> Result<bool> {
    let conn = Connection::connect(conn_str, TlsMode::None)
        .map_err(|e| format_err!(ErrorKind::Migration, "{}", e))?;
    let rows = conn.query(sql::PG_MIGRATION_TABLE_EXISTS, &[])
        .map_err(|e| format_err!(ErrorKind::Migration, "{}", e))?;
    let exists: bool = rows.iter().next().unwrap().get(0);
    Ok(exists)
}


// --
// Create `__migrant_migrations` table
// --
#[cfg(not(feature="postgresql"))]
pub fn migration_setup(conn_str: &str) -> Result<bool> {
    if !migration_table_exists(conn_str)? {
        psql_cmd(conn_str, sql::CREATE_TABLE)?;
        return Ok(true)
    }
    Ok(false)
}

#[cfg(feature="postgresql")]
pub fn migration_setup(conn_str: &str) -> Result<bool> {
    if !migration_table_exists(conn_str)? {
        let conn = Connection::connect(conn_str, TlsMode::None)
            .map_err(|e| format_err!(ErrorKind::Migration, "{}", e))?;
        conn.execute(sql::CREATE_TABLE, &[])
            .map_err(|e| format_err!(ErrorKind::Migration, "{}", e))?;
        return Ok(true)
    }
    Ok(false)
}


// --
// Select all migrations from `__migrant_migrations` table
// --
#[cfg(not(feature="postgresql"))]
pub fn select_migrations(conn_str: &str) -> Result<Vec<String>> {
    let stdout = psql_cmd(conn_str, sql::GET_MIGRATIONS)?;
    Ok(stdout.trim().lines().map(String::from).collect())
}

#[cfg(feature="postgresql")]
pub fn select_migrations(conn_str: &str) -> Result<Vec<String>> {
    let conn = Connection::connect(conn_str, TlsMode::None)?;
    let rows = conn.query(sql::GET_MIGRATIONS, &[])?;
    Ok(rows.iter().map(|row| row.get(0)).collect())
}


// --
// Insert migration tag into `__migrant_migrations` table
// --
#[cfg(not(feature="postgresql"))]
pub fn insert_migration_tag(conn_str: &str, tag: &str) -> Result<()> {
    psql_cmd(conn_str, &sql::PG_ADD_MIGRATION.replace("__VAL__", tag))?;
    Ok(())
}

#[cfg(feature="postgresql")]
pub fn insert_migration_tag(conn_str: &str, tag: &str) -> Result<()> {
    let conn = Connection::connect(conn_str, TlsMode::None)?;
    conn.execute("insert into __migrant_migrations (tag) values ($1)", &[&tag])?;
    Ok(())
}


// --
// Delete migration tag from `__migrant_migrations` table
// --
#[cfg(not(feature="postgresql"))]
pub fn remove_migration_tag(conn_str: &str, tag: &str) -> Result<()> {
    psql_cmd(conn_str, &sql::PG_DELETE_MIGRATION.replace("__VAL__", tag))?;
    Ok(())
}

#[cfg(feature="postgresql")]
pub fn remove_migration_tag(conn_str: &str, tag: &str) -> Result<()> {
    let conn = Connection::connect(conn_str, TlsMode::None)?;
    conn.execute("delete from __migrant_migrations where tag = $1", &[&tag])?;
    Ok(())
}


// --
// Apply migration to database
// --
#[cfg(not(feature="postgresql"))]
pub fn run_migration(conn_str: &str, filename: &str) -> Result<()> {
    let migrate = Command::new("psql")
            .arg(&conn_str)
            .arg("-f").arg(filename)
            .output()?;
    if !migrate.status.success() {
        let stderr = std::str::from_utf8(&migrate.stderr)?;
        bail_fmt!(ErrorKind::Migration, "Error executing statement, stderr: `{}`", stderr);
    }
    Ok(())
}

#[cfg(feature="postgresql")]
pub fn run_migration(conn_str: &str, filename: &str) -> Result<()> {
    let mut file = std::fs::File::open(filename)?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;

    let conn = Connection::connect(conn_str, TlsMode::None)
        .map_err(|e| format_err!(ErrorKind::Migration, "{}", e))?;
    conn.batch_execute(&buf)
        .map_err(|e| format_err!(ErrorKind::Migration, "{}", e))?;
    Ok(())
}


#[cfg(test)]
mod test {
    use std;
    use super::*;
    macro_rules! _try {
        ($exp:expr) => {
            match $exp {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Caught: {}", e);
                    panic!(e)
                }
            }
        }
    }

    #[test]
    fn postgres() {
        let conn_str = std::env::var("POSTGRES_TEST_CONN_STR").unwrap();

        // no table before setup
        let is_setup = _try!(migration_table_exists(&conn_str));
        assert_eq!(false, is_setup);

        // setup migration table
        let was_setup = _try!(migration_setup(&conn_str));
        assert_eq!(true, was_setup);
        let was_setup = _try!(migration_setup(&conn_str));
        assert_eq!(false, was_setup);

        // table exists after setup
        let is_setup = _try!(migration_table_exists(&conn_str));
        assert!(is_setup);

        // insert some tags
        _try!(insert_migration_tag(&conn_str, "initial"));
        _try!(insert_migration_tag(&conn_str, "alter1"));
        _try!(insert_migration_tag(&conn_str, "alter2"));

        // get applied
        let migs = _try!(select_migrations(&conn_str));
        assert_eq!(3, migs.len());

        // remove some tags
        _try!(remove_migration_tag(&conn_str, "alter2"));
        let migs = _try!(select_migrations(&conn_str));
        assert_eq!(2, migs.len());
    }
}
