/*!
When using migrant as a library, migrations can be defined in the source code
instead of being discovered from the file system. This also provides the
option of creating programmable migrations in rust!

This example shows configuration/functionality for sqlite. Using postgres or mysql
would require using the appropriate `Settings::configure_<db>` method, enabling
the corresponding database feature, and importing the required database connection crate.

NOTE: The feature-gates are only required here so the example will compile when running
      tests with and without features. In regular usage, the `cfg`s are not required since
      the specified database feature should be enabled in your `Cargo.toml` entry.

This should be run with `cargo run --example embedded_programmable --features d-sqlite`

*/
extern crate migrant_lib;
#[cfg(feature = "d-sqlite")]
extern crate rusqlite;

#[cfg(feature = "d-sqlite")]
use migrant_lib::{
    Config, ConnConfig, Direction, EmbeddedMigration, FileMigration, FnMigration, Migrator,
    Settings,
};
#[cfg(feature = "d-sqlite")]
use rusqlite::types::ToSql;
#[cfg(feature = "d-sqlite")]
use std::env;

#[cfg(feature = "d-sqlite")]
mod migrations {
    use super::*;
    pub struct AddUserData;

    impl AddUserData {
        pub fn up(config: ConnConfig) -> Result<(), Box<dyn std::error::Error>> {
            let db_path = config.database_path()?;
            let conn = rusqlite::Connection::open(&db_path)?;
            let people = ["james", "lauren", "bean"];
            for (i, name) in people.iter().enumerate() {
                let id = i as u32 + 1;
                conn.execute(
                    "insert into users (id, name) values (?1, ?2);",
                    &[&id as &dyn ToSql, name],
                )?;
            }
            Ok(())
        }
        pub fn down(config: ConnConfig) -> Result<(), Box<dyn std::error::Error>> {
            let db_path = config.database_path()?;
            let conn = rusqlite::Connection::open(&db_path)?;
            let people = ["james", "lauren", "bean"];
            for name in &people {
                conn.execute("delete from users where name = ?1", &[name])?;
            }
            Ok(())
        }
    }
}

#[cfg(feature = "d-sqlite")]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::current_dir()?;
    let path = path.join("db/embedded_example.db");
    let settings = Settings::configure_sqlite().database_path(&path)?.build()?;

    let mut config = Config::with_settings(&settings);
    config.setup()?;

    // Define migrations
    config.use_migrations(&[
        EmbeddedMigration::with_tag("create-users-table")
            .up(include_str!(
                "../migrations/embedded/create_users_table/up.sql"
            ))
            .down(include_str!(
                "../migrations/embedded/create_users_table/down.sql"
            ))
            .boxed(),
        FnMigration::with_tag("add-user-data")
            .up(migrations::AddUserData::up)
            .down(migrations::AddUserData::down)
            .boxed(),
        FileMigration::with_tag("create-places-table")
            .up("migrations/embedded/create_places_table/up.sql")?
            .down("migrations/embedded/create_places_table/down.sql")?
            .boxed(),
        EmbeddedMigration::with_tag("alter-places-table-add-address")
            .up(String::from("alter table places add column address text;"))
            .down(
                "create table new_places (name text);\
                   insert into new_places select name from places;\
                   drop table if exists places;
                   alter table new_places rename to places;",
            )
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
