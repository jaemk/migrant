use std::fs;
use std::path::Path;
use super::*;

#[cfg(feature="sqlite")]
use std::io::Read;
#[cfg(feature="sqlite")]
use rusqlite::Connection;

#[cfg(not(feature="sqlite"))]
use std::process::Command;
#[cfg(not(feature="sqlite"))]
use std::str;

// --
// Check database exists / create it
// --
/// Create a file if it doesn't exist, returning true if the file was created
pub fn create_file_if_missing(path: &Path) -> Result<bool> {
    if path.exists() {
        Ok(false)
    } else {
        let db_dir = path.parent().unwrap();
        fs::create_dir_all(db_dir).map_err(Error::IoCreate)?;
        println!("{:?}", path);
        fs::File::create(path).map_err(Error::IoCreate)?;
        Ok(true)
    }
}


#[cfg(not(feature="sqlite"))]
fn sqlite_cmd(db_path: &str, cmd: &str) -> Result<String> {
    let out = Command::new("sqlite3")
                    .arg(&db_path)
                    .arg("-csv")
                    .arg(cmd)
                    .output()
                    .map_err(Error::IoProc)?;
    if !out.status.success() {
        let stderr = str::from_utf8(&out.stderr).unwrap();
        bail!(Migration <- "Error executing statement, stderr: `{}`", stderr);
    }
    let stdout = String::from_utf8(out.stdout)?;
    Ok(stdout)
}


// --
// Check `__migrant_migrations` table exists
// --
#[cfg(not(feature="sqlite"))]
pub fn migration_table_exists(db_path: &str) -> Result<bool> {
    let stdout = sqlite_cmd(db_path, sql::SQLITE_MIGRATION_TABLE_EXISTS)?;
    Ok(stdout.trim() == "1")
}

#[cfg(feature="sqlite")]
pub fn migration_table_exists(db_path: &str) -> Result<bool> {
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
        sqlite_cmd(db_path, sql::CREATE_TABLE)?;
        return Ok(true)
    }
    Ok(false)
}

#[cfg(feature="sqlite")]
pub fn migration_setup(db_path: &Path) -> Result<bool> {
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
    let stdout = sqlite_cmd(db_path, sql::GET_MIGRATIONS)?;
    Ok(stdout.trim().lines().map(String::from).collect::<Vec<_>>())
}

#[cfg(feature="sqlite")]
pub fn select_migrations(db_path: &str) -> Result<Vec<String>> {
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
    sqlite_cmd(db_path, &sql::SQLITE_ADD_MIGRATION.replace("__VAL__", tag))?;
    Ok(())
}

#[cfg(feature="sqlite")]
pub fn insert_migration_tag(db_path: &str, tag: &str) -> Result<()> {
    let conn = Connection::open(db_path)?;
    conn.execute("insert into __migrant_migrations (tag) values ($1)", &[&tag])?;
    Ok(())
}


// --
// Remove tag from `__migrant_migrations` table
// --
#[cfg(not(feature="sqlite"))]
pub fn remove_migration_tag(db_path: &str, tag: &str) -> Result<()> {
    sqlite_cmd(db_path, &sql::SQLITE_DELETE_MIGRATION.replace("__VAL__", tag))?;
    Ok(())
}

#[cfg(feature="sqlite")]
pub fn remove_migration_tag(db_path: &str, tag: &str) -> Result<()> {
    let conn = Connection::open(db_path)?;
    conn.execute("delete from __migrant_migrations where tag = $1", &[&tag])?;
    Ok(())
}


// --
// Apply migration file to database
// --
#[cfg(not(feature="sqlite"))]
pub fn run_migration(db_path: &Path, filename: &str) -> Result<()> {
    let db_path = db_path.to_str().unwrap();
    sqlite_cmd(db_path, &format!(".read {}", filename))?;
    Ok(())
}

#[cfg(feature="sqlite")]
pub fn run_migration(db_path: &Path, filename: &str) -> Result<()> {
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
    fn sqlite() {
        let conn_str = std::env::var("SQLITE_TEST_CONN_STR").unwrap();
        let path = std::path::Path::new(&conn_str);

        // no table before setup
        let is_setup = _try!(migration_table_exists(&conn_str));
        assert_eq!(false, is_setup);

        // setup migration table
        let was_setup = _try!(migration_setup(&path));
        assert_eq!(true, was_setup);
        let was_setup = _try!(migration_setup(&path));
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
