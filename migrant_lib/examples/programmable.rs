/*!
When using migrant as a library, migrations can be defined in the source code
instead of being discovered from the file system. This also provides the
option of creating programmable migrations in rust!

*/
extern crate migrant_lib;

use std::env;
use migrant_lib::{Config, FileMigration, FnMigration, Migrator, Direction};


mod migrations {
    use super::*;
    pub struct Custom;
    #[cfg(not(feature="sqlite"))]
    impl Custom {
        pub fn up(_: migrant_lib::DbConn) -> Result<(), Box<std::error::Error>> {
            print!(" <Up!>");
            Ok(())
        }
        pub fn down(_: migrant_lib::DbConn) -> Result<(), Box<std::error::Error>> {
            print!(" <Down!>");
            Ok(())
        }
    }

    #[cfg(feature="sqlite")]
    impl Custom {
        // Postgres
        // pub fn up(conn: migrant_lib::DbConn) -> Result<(), Box<std::error::Error>> {
        //     let conn = conn.pg_connection()?;
        //     let rows = conn.query("select random() * 100 from generate_series(1,1)", &[])?;
        //     for row in &rows {
        //         let n: f32 = row.get(0);
        //         print!(" <{:?}>", n);
        //     }
        //     Ok(())
        // }
        // pub fn down(conn: migrant_lib::DbConn) -> Result<(), Box<std::error::Error>> {
        //     let _conn = conn.pg_connection()?;
        //     Ok(())
        // }

        /// Sqlite
        pub fn up(conn: migrant_lib::DbConn) -> Result<(), Box<std::error::Error>> {
            let conn = conn.sqlite_connection()?;
            conn.query_row("select abs(random() % 100)", &[], |row| {
                let n: u32 = row.get(0);
                print!(" <{:?}>", n);
            })?;
            Ok(())
        }
        pub fn down(conn: migrant_lib::DbConn) -> Result<(), Box<std::error::Error>> {
            let _conn = conn.sqlite_connection()?;
            Ok(())
        }
    }
}


fn run() -> Result<(), Box<std::error::Error>> {
    let dir = env::current_dir().unwrap();
    let mut config = match migrant_lib::search_for_config(&dir) {
        // Setup a migrant configuration if it doesn't exist
        None => {
            Config::init_in(&dir)
                .initialize()?;
            return Ok(())
        }

        // Load config file, but don't ping the database for applied migrations.
        // We need to define our migrations first so our `Config` knows
        // that we're using explicitly defined migrations (with arbitrary tags)
        // instead of auto-generated migrations (with a strict tag format).
        Some(p) => Config::load_file_only(&p)?
    };

    // Define migrations
    config.use_migrations(vec![
        FileMigration::with_tag("initial")?
            .up("migrations/initial/up.sql")?
            .down("migrations/initial/down.sql")?
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
