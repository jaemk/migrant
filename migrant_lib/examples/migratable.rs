/*!
When using migrant as a library, migrations can be defined in the source code
instead of being discovered from the file system.

*/
extern crate migrant_lib;

use std::env;
use migrant_lib::{Config, FileMigration, Migrator, Direction};


fn run() -> Result<(), Box<std::error::Error>> {
    let dir = env::current_dir().unwrap();
    let mut config = match migrant_lib::search_for_config(&dir) {
        None => {
            Config::init_in(&dir)
                .initialize()?;
            return Ok(())
        }
        Some(p) => Config::load_file_only(&p)?
    };

    config.use_migrations(vec![
        FileMigration::with_tag("initial")?
            .up("migrations/initial/up.sql")?
            .down("migrations/initial/down.sql")?
            .boxed(),
        FileMigration::with_tag("second")?
            .up("migrations/second/up.sql")?
            .down("migrations/second/down.sql")?
            .boxed(),
    ])?;
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

    println!("Unapplying migrations...");
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
