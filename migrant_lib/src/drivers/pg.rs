/// Postgres database functions using shell commands and db drivers
use super::*;

#[cfg(feature="postgresql")]
use std::io::Read;

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
        bail!(Migration <- "Error executing statement, stderr: `{}`", stderr);
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
            bail!(Migration <- "Error executing statement, stderr: `{}`", stderr);
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
        bail!(Migration <- "Error executing statement, stderr: `{}`", stderr);
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
        bail!(Migration <- "Error executing statement, stderr: `{}`", stderr);
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
    let remove = Command::new("psql")
                    .arg(conn_str)
                    .arg("-t")      // no headers or footer
                    .arg("-A")      // un-aligned output
                    .arg("-F,")     // comma separator
                    .arg("-c")
                    .arg(sql::PG_DELETE_MIGRATION.replace("__VAL__", tag))
                    .output()
                    .map_err(Error::IoProc)?;
    if !remove.status.success() {
        let stderr = std::str::from_utf8(&remove.stderr).unwrap();
        bail!(Migration <- "Error executing statement, stderr: `{}`", stderr);
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
#[cfg(not(feature="postgresql"))]
pub fn run_migration(conn_str: &str, filename: &str) -> Result<()> {
    let migrate = Command::new("psql")
            .arg(&conn_str)
            .arg("-f").arg(filename)
            .output()
            .map_err(Error::IoProc)?;
    if !migrate.status.success() {
        let stderr = std::str::from_utf8(&migrate.stderr).unwrap();
        bail!(Migration <- "Error executing statement, stderr: `{}`", stderr);
    }
    Ok(())
}

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

