/*!
In-memory sqlite databases are selected with the special `:memory:` path
(or `Settings::configure_sqlite().memory()`). The database connection is
established once and kept alive by the `Config` -- and shared by all of its
clones -- so migrations and application queries all operate on the same
database. Use `Config::sqlite_connection` (or `ConnConfig::sqlite_connection`
inside function-migrations) to access the live connection.

This should be run with `cargo run --example in_memory_sqlite --features d-sqlite`
*/

#[cfg(feature = "d-sqlite")]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use migrant_lib::{Config, ConnConfig, EmbeddedMigration, FnMigration, Migrator, Settings};

    fn seed(conn: ConnConfig) -> Result<(), Box<dyn std::error::Error>> {
        let handle = conn.sqlite_connection()?;
        let conn = handle.lock().unwrap();
        for name in ["james", "lauren", "bean"] {
            conn.execute("insert into users (name) values (?1)", [name])?;
        }
        Ok(())
    }

    let settings = Settings::configure_sqlite().memory().build()?;
    let mut config = Config::with_settings(&settings);
    config.setup()?;

    config.use_migrations(&[
        EmbeddedMigration::with_tag("create-users-table")
            .up("create table users (id integer primary key, name text);")
            .down("drop table users;")
            .boxed(),
        FnMigration::with_tag("seed-users")
            .up(seed)
            .down(migrant_lib::migration::noop)
            .boxed(),
    ])?;

    let config = config.reload()?;
    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .swallow_completion(true)
        .apply()?;

    // The application can now use the same live in-memory database
    let handle = config.sqlite_connection()?;
    let conn = handle.lock().unwrap();
    let count: i64 = conn.query_row("select count(*) from users", [], |row| row.get(0))?;
    println!("users in the in-memory database: {}", count);
    Ok(())
}

#[cfg(not(feature = "d-sqlite"))]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    Err("d-sqlite database feature required".into())
}

pub fn main() {
    if let Err(e) = run() {
        println!("[ERROR] {}", e);
    }
}
