//! Regression test for `Config::reload` on a settings-file config backed by an
//! in-memory sqlite database (`database_path = ":memory:"`).
//!
//! `reload` rebuilds the config from the settings file, which previously
//! produced a fresh, unconnected config and silently discarded the live
//! in-memory database. The applied migrations (and any data) must survive a
//! reload when the settings on disk are unchanged.
#![cfg(feature = "sqlite")]

use std::io::Write;

use migrant_lib::{Config, EmbeddedMigration, Migrator};

fn write_memory_settings() -> tempfile::NamedTempFile {
    let mut file = tempfile::Builder::new()
        .prefix("Migrant")
        .suffix(".toml")
        .tempfile()
        .unwrap();
    file.write_all(
        b"database_type = \"sqlite\"\n\
          database_path = \":memory:\"\n",
    )
    .unwrap();
    file.flush().unwrap();
    file
}

fn configure(path: &std::path::Path) -> Config {
    let mut config = Config::from_settings_file(path).unwrap();
    config
        .use_migrations(&[EmbeddedMigration::with_tag("create-users")
            .up("create table users (id integer primary key, name text);")
            .down("drop table users;")
            .boxed()])
        .unwrap();
    config
}

fn applied_tags(config: &Config) -> Vec<String> {
    migrant_lib::migration_statuses(config)
        .unwrap()
        .into_iter()
        .filter(|m| m.applied)
        .map(|m| m.tag)
        .collect()
}

#[test]
fn reload_preserves_in_memory_database() {
    let settings_file = write_memory_settings();
    let config = configure(settings_file.path());
    config.setup().unwrap();

    // reload right after setup: the migrations table lives only in the
    // in-memory connection, so it must be carried across the reload.
    let config = config.reload().unwrap();

    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();

    // write some application data through the same live connection
    {
        let handle = config.sqlite_connection().unwrap();
        let conn = handle.lock().unwrap();
        conn.execute("insert into users (name) values (?1)", ["james"])
            .unwrap();
    }

    // reload again: applied migration list and data must both survive
    let config = config.reload().unwrap();
    assert_eq!(vec!["create-users".to_string()], applied_tags(&config));

    let handle = config.sqlite_connection().unwrap();
    let conn = handle.lock().unwrap();
    let n: i64 = conn
        .query_row("select count(*) from users", [], |row| row.get(0))
        .unwrap();
    assert_eq!(1, n, "data written before reload must survive the reload");
}
