/*!
This example shows functionality similar to the `embedded_programmable` example but
with configuration read from a settings file at runtime (similar to `migrant_cli_compatible`).

NOTE: The feature-gates are only required here so the example will compile when running
      tests with and without features. In regular usage, the `cfg`s are not required since
      the specified database feature should be enabled in your `Cargo.toml` entry.

This should be run with `cargo run --example settings_file_programmable --features d-sqlite`
*/
extern crate migrant_lib;

#[cfg(feature = "d-sqlite")]
use migrant_lib::config::SqliteSettingsBuilder;
#[cfg(feature = "d-sqlite")]
use migrant_lib::{Config, Direction, EmbeddedMigration, Migrator};
#[cfg(feature = "d-sqlite")]
use std::env;

#[cfg(feature = "d-sqlite")]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let dir = env::current_dir().unwrap();
    let mut config = match migrant_lib::search_for_settings_file(&dir) {
        None => {
            Config::init_in(&dir)
                .with_sqlite_options(
                    SqliteSettingsBuilder::empty()
                        .database_path("db/db.db")?
                        .migration_location("migrations/managed")?,
                )
                .initialize()?;
            println!(
                "\nSettings file and migrations table initialized. \
                 Please run again to apply migrations."
            );
            return Ok(());
        }
        Some(p) => Config::from_settings_file(&p)?,
    };

    // Define migrations
    config.use_migrations(&[
        EmbeddedMigration::with_tag("create-users-table")
            .up("create table users (id integer primary key, name text);")
            .down("drop table users;")
            .boxed(),
        EmbeddedMigration::with_tag("create-places-table")
            .up("create table places (id integer primary key, address text);")
            .down("drop table places;")
            .boxed(),
    ])?;

    // Reload config, ping the database for applied migrations
    let config = config.reload()?;

    println!("Applying migrations...");
    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .swallow_completion(true)
        .apply()?;

    let config = config.reload()?;
    migrant_lib::list(&config)?;

    println!("\nUnapplying migrations...");
    Migrator::with_config(&config)
        .all(true)
        .direction(Direction::Down)
        .swallow_completion(true)
        .apply()?;

    let config = config.reload()?;
    migrant_lib::list(&config)?;
    Ok(())
}

#[cfg(not(feature = "d-sqlite"))]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    Err("d-sqlite database feature required")?;
    Ok(())
}

pub fn main() {
    if let Err(e) = run() {
        println!("[ERROR] {}", e);
    }
}
