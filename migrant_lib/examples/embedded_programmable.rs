/*!
When using migrant as a library, migrations can be defined in the source code
instead of being discovered from the file system. This also provides the
option of creating programmable migrations in rust!

This example shows functionality for both sqlite and postgres databases, but
has a `Settings` object configured to run only for sqlite.
This should be run with `cargo run --example embedded_programmable`

*/
extern crate migrant_lib;

use migrant_lib::{Config, Settings, DbKind, FileMigration, EmbeddedMigration, FnMigration, Migrator, Direction};


mod migrations {
    use super::*;
    pub struct Custom;
    #[cfg(any( not(any(feature="sqlite", feature="postgresql")), all(feature="sqlite", feature="postgresql")))]
    impl Custom {
        pub fn up(_: migrant_lib::DbConn) -> Result<(), Box<std::error::Error>> {
            print!(" <[Up] Hint: Use a (only one) database specific feature!>");
            Ok(())
        }
        pub fn down(_: migrant_lib::DbConn) -> Result<(), Box<std::error::Error>> {
            print!(" <[Down] Hint: Use a (only one) database specific feature!>");
            Ok(())
        }
    }

    #[cfg(all(feature="sqlite", not(feature="postgresql")))]
    impl Custom {
        /// Sqlite
        pub fn up(conn: migrant_lib::DbConn) -> Result<(), Box<std::error::Error>> {
            let conn = conn.sqlite_connection()?;
            conn.query_row("select abs(random() % 100)", &[], |row| {
                let n: u32 = row.get(0);
                print!(" <random number: {:?}>", n);
            })?;
            Ok(())
        }
        pub fn down(conn: migrant_lib::DbConn) -> Result<(), Box<std::error::Error>> {
            let _conn = conn.sqlite_connection()?;
            Ok(())
        }
    }

    #[cfg(all(feature="postgresql", not(feature="sqlite")))]
    impl Custom {
        /// Postgres
        pub fn up(conn: migrant_lib::DbConn) -> Result<(), Box<std::error::Error>> {
            let conn = conn.pg_connection()?;
            let rows = conn.query("select (select random() * 100 from generate_series(1,1))::int", &[])?;
            for row in &rows {
                let n: i32 = row.get(0);
                print!(" <random number: {:?}>", n);
            }
            Ok(())
        }
        pub fn down(conn: migrant_lib::DbConn) -> Result<(), Box<std::error::Error>> {
            let _conn = conn.pg_connection()?;
            Ok(())
        }
    }
}


/// Migrant will normally handle creating a database file if it's missing.
/// This is just so we can use `Path::canonicalize` to get the absolute
/// path since it can't be hardcoded for this example.
pub fn create_file_if_missing(path: &std::path::Path) -> Result<bool, Box<std::error::Error>> {
    if path.exists() {
        Ok(false)
    } else {
        let db_dir = path.parent()
            .ok_or_else(|| format!("Unable to determine parent path: {:?}", path))?;
        std::fs::create_dir_all(db_dir)?;
        std::fs::File::create(path)?;
        Ok(true)
    }
}


fn run() -> Result<(), Box<std::error::Error>> {
    let path = std::path::Path::new("db/db.db");
    create_file_if_missing(path)?;
    let path = path.canonicalize()?;
    let mut settings = Settings::with_db_type(DbKind::Sqlite);
    settings.database_path(&path)?;

    let mut config = Config::with_settings(&settings);
    config.setup()?;

    // Define migrations
    config.use_migrations(vec![
        EmbeddedMigration::with_tag("initial")?
            .up(include_str!("../migrations/initial/up.sql"))
            .down(include_str!("../migrations/initial/down.sql"))
            .boxed(),
        FileMigration::with_tag("second")?
            .up("migrations/second/up.sql")?
            .down("migrations/second/down.sql")?
            .boxed(),
        FnMigration::with_tag("custom")?
            .up(migrations::Custom::up)
            .down(migrations::Custom::down)
            .boxed(),
    ])?;

    // Reload config, ping the database for applied migrations
    let config = config.reload()?;

    println!("Applying migrations...");
    let res = Migrator::with_config(&config)
        .all(true)
        .apply();
    match res {
        Err(ref e) if e.is_migration_complete() => (),
        res => res?,
    }

    let config = config.reload()?;
    migrant_lib::list(&config)?;

    println!("\nUnapplying migrations...");
    let res = Migrator::with_config(&config)
        .all(true)
        .direction(Direction::Down)
        .apply();
    match res {
        Err(ref e) if e.is_migration_complete() => (),
        res => res?,
    }

    let config = config.reload()?;
    migrant_lib::list(&config)?;
    Ok(())
}

pub fn main() {
    if let Err(e) = run() {
        println!("[ERROR] {}", e);
    }
}
