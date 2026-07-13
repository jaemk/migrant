//! End-to-end sqlite tests, covering in-memory databases where the
//! connection handle must be kept alive across all operations.
#![cfg(feature = "d-sqlite")]

use migrant_lib::{
    Config, ConnConfig, Direction, EmbeddedMigration, FnMigration, Migrator, Settings,
};

fn seed_users(conn: ConnConfig) -> Result<(), Box<dyn std::error::Error>> {
    let handle = conn.sqlite_connection()?;
    let conn = handle.lock().unwrap();
    conn.execute("insert into users (name) values (?1)", ["james"])?;
    Ok(())
}

fn unseed_users(conn: ConnConfig) -> Result<(), Box<dyn std::error::Error>> {
    let handle = conn.sqlite_connection()?;
    let conn = handle.lock().unwrap();
    conn.execute("delete from users where name = ?1", ["james"])?;
    Ok(())
}

fn migrations_config(settings: &Settings) -> Config {
    let mut config = Config::with_settings(settings);
    config
        .use_migrations(&[
            EmbeddedMigration::with_tag("create-users")
                .up("create table users (id integer primary key, name text);")
                .down("drop table users;")
                .boxed(),
            FnMigration::with_tag("seed-users")
                .up(seed_users)
                .down(unseed_users)
                .boxed(),
        ])
        .unwrap();
    config
}

fn user_count(config: &Config) -> i64 {
    let handle = config.sqlite_connection().unwrap();
    let conn = handle.lock().unwrap();
    conn.query_row("select count(*) from users", [], |row| row.get(0))
        .unwrap()
}

fn applied_tags(config: &Config) -> Vec<String> {
    migrant_lib::migration_statuses(config)
        .unwrap()
        .into_iter()
        .filter(|m| m.applied)
        .map(|m| m.tag)
        .collect()
}

fn table_exists(config: &Config, name: &str) -> bool {
    let handle = config.sqlite_connection().unwrap();
    let conn = handle.lock().unwrap();
    conn.query_row(
        "select exists(select 1 from sqlite_master where type = 'table' and name = ?1)",
        [name],
        |row| row.get(0),
    )
    .unwrap()
}

/// Build an in-memory config with a single migration whose `up` creates a table
/// and then runs an invalid statement, so application fails partway through.
fn failing_migration_config(no_transaction: bool) -> Config {
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let mut migration = EmbeddedMigration::with_tag("bad");
    migration
        .up("create table good (x integer); insert into does_not_exist values (1);")
        .down("drop table good;");
    if no_transaction {
        migration.no_transaction();
    }
    let mut config = Config::with_settings(&settings);
    config.use_migrations(&[migration.boxed()]).unwrap();
    config
}

#[test]
fn in_memory_database_end_to_end() {
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let config = migrations_config(&settings);
    config.setup().unwrap();
    let config = config.reload().unwrap();

    // apply everything
    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .swallow_completion(true)
        .apply()
        .unwrap();

    // the same live connection sees the migrated schema and data
    let config = config.reload().unwrap();
    assert_eq!(
        vec!["create-users".to_string(), "seed-users".to_string()],
        applied_tags(&config)
    );
    assert_eq!(1, user_count(&config));

    // un-apply everything; the fn-migration's `down` runs on the same db
    Migrator::with_config(&config)
        .all(true)
        .direction(Direction::Down)
        .show_output(false)
        .swallow_completion(true)
        .apply()
        .unwrap();

    let config = config.reload().unwrap();
    assert!(applied_tags(&config).is_empty());
}

#[test]
fn in_memory_database_shared_across_clones() {
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let config = migrations_config(&settings);
    config.setup().unwrap();

    let clone = config.clone();
    {
        let handle = config.sqlite_connection().unwrap();
        let conn = handle.lock().unwrap();
        conn.execute_batch("create table t(x integer); insert into t values (1);")
            .unwrap();
    }
    let handle = clone.sqlite_connection().unwrap();
    let conn = handle.lock().unwrap();
    let n: i64 = conn
        .query_row("select count(*) from t", [], |row| row.get(0))
        .unwrap();
    assert_eq!(1, n, "clones share the same in-memory database");
}

#[test]
fn failed_migration_rolls_back_atomically() {
    let config = failing_migration_config(false);
    config.setup().unwrap();
    let config = config.reload().unwrap();

    let res = Migrator::with_config(&config).show_output(false).apply();
    assert!(res.is_err(), "a migration with invalid sql must fail");

    let config = config.reload().unwrap();
    // The whole migration was wrapped in a transaction: the partial `create
    // table` is rolled back and the bookkeeping row is never written.
    assert!(
        !table_exists(&config, "good"),
        "partial DDL must be rolled back"
    );
    assert!(
        applied_tags(&config).is_empty(),
        "the tag must not be recorded when the migration fails"
    );
}

#[test]
fn no_transaction_migration_leaves_partial_state() {
    let config = failing_migration_config(true);
    config.setup().unwrap();
    let config = config.reload().unwrap();

    let res = Migrator::with_config(&config).show_output(false).apply();
    assert!(res.is_err(), "a migration with invalid sql must fail");

    let config = config.reload().unwrap();
    // With `no_transaction`, the earlier `create table` is not rolled back...
    assert!(
        table_exists(&config, "good"),
        "without a transaction the create persists"
    );
    // ...but a failed migration is still never recorded as applied.
    assert!(
        applied_tags(&config).is_empty(),
        "the tag must not be recorded when the migration fails"
    );
}

#[test]
fn file_database_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let settings = Settings::configure_sqlite()
        .database_path(&db_path)
        .unwrap()
        .build()
        .unwrap();
    let config = migrations_config(&settings);
    config.setup().unwrap();
    assert!(db_path.exists(), "setup creates the database file");

    let config = config.reload().unwrap();
    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .swallow_completion(true)
        .apply()
        .unwrap();

    let config = config.reload().unwrap();
    assert_eq!(2, applied_tags(&config).len());
    assert_eq!(1, user_count(&config));
}
