//! End-to-end tests against server databases (postgres, mysql).
//!
//! These are skipped unless the corresponding connection string is provided:
//! `POSTGRES_TEST_CONN_STR` (e.g. `postgres://user:pass@localhost:5432/db`)
//! and `MYSQL_TEST_CONN_STR` (e.g. `mysql://user:pass@localhost:3306/db`).
#![cfg(any(feature = "d-postgres", feature = "d-mysql"))]

use migrant_lib::{Config, Direction, EmbeddedMigration, Migrator, Settings};

struct ConnParts {
    name: String,
    user: String,
    password: String,
    host: String,
    port: u16,
}

fn parse_conn_str(conn_str: &str, default_port: u16) -> ConnParts {
    let url = url::Url::parse(conn_str).expect("invalid test connection string");
    ConnParts {
        name: url.path().trim_start_matches('/').to_string(),
        user: url.username().to_string(),
        password: url.password().unwrap_or_default().to_string(),
        host: url.host_str().unwrap_or("localhost").to_string(),
        port: url.port().unwrap_or(default_port),
    }
}

fn apply_and_unapply(settings: &Settings) {
    let mut config = Config::with_settings(settings);
    config.setup().unwrap();
    config
        .use_migrations(&[
            EmbeddedMigration::with_tag("create-users")
                .up("create table users (name varchar(64));")
                .down("drop table users;")
                .boxed(),
            EmbeddedMigration::with_tag("seed-users")
                .up("insert into users (name) values ('james');")
                .down("delete from users where name = 'james';")
                .boxed(),
        ])
        .unwrap();

    // reset any state left over from a previous (failed) run
    let config = config.reload().unwrap();
    Migrator::with_config(&config)
        .direction(Direction::Down)
        .all(true)
        .show_output(false)
        .swallow_completion(true)
        .apply()
        .unwrap();

    let config = config.reload().unwrap();
    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();

    let config = config.reload().unwrap();
    let statuses = migrant_lib::migration_statuses(&config).unwrap();
    assert_eq!(2, statuses.len());
    assert!(statuses.iter().all(|m| m.applied));

    Migrator::with_config(&config)
        .direction(Direction::Down)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();

    let config = config.reload().unwrap();
    let statuses = migrant_lib::migration_statuses(&config).unwrap();
    assert!(statuses.iter().all(|m| !m.applied));
}

/// Drop the migration table so the next run starts from a clean database.
#[cfg(feature = "d-postgres")]
fn drop_pg_migration_table(conn_str: &str) {
    let mut client = postgres::Client::connect(conn_str, postgres::NoTls)
        .expect("connect to drop postgres migration table");
    client
        .batch_execute("drop table if exists __migrant_migrations;")
        .expect("drop postgres migration table");
}

/// Drop the migration table so the next run starts from a clean database.
#[cfg(feature = "d-mysql")]
fn drop_mysql_migration_table(conn_str: &str) {
    use mysql::prelude::Queryable;
    let opts = mysql::Opts::from_url(conn_str).expect("parse mysql connection string");
    let mut conn = mysql::Conn::new(opts).expect("connect to drop mysql migration table");
    conn.query_drop("drop table if exists __migrant_migrations;")
        .expect("drop mysql migration table");
}

#[cfg(feature = "d-postgres")]
#[test]
fn postgres_end_to_end() {
    let conn_str = match std::env::var("POSTGRES_TEST_CONN_STR") {
        Ok(s) => s,
        Err(_) => {
            eprintln!("POSTGRES_TEST_CONN_STR not set, skipping");
            return;
        }
    };
    let parts = parse_conn_str(&conn_str, 5432);
    let settings = Settings::configure_postgres()
        .database_name(&parts.name)
        .database_user(&parts.user)
        .database_password(&parts.password)
        .database_host(&parts.host)
        .database_port(parts.port)
        .build()
        .unwrap();
    // drop any leftover table from an earlier interrupted run
    drop_pg_migration_table(&conn_str);
    apply_and_unapply(&settings);
    drop_pg_migration_table(&conn_str);
    // atomic-rollback phase runs against the same database (see the helper doc)
    assert_failed_migration_rolls_back(&conn_str, &settings);
    drop_pg_migration_table(&conn_str);
    // force-past-failure phase, also against the same database
    assert_force_continues_holding_lock(&conn_str, &settings);
    drop_pg_migration_table(&conn_str);
}

/// A migration whose SQL fails partway is rolled back atomically on postgres:
/// the partial DDL is undone and the bookkeeping row is never written.
///
/// Not a standalone `#[test]`: it shares the one postgres database (and the
/// single `__migrant_migrations` table) with `postgres_end_to_end`, so it runs
/// as a phase of that test rather than racing it under cargo's parallel runner.
#[cfg(feature = "d-postgres")]
fn assert_failed_migration_rolls_back(conn_str: &str, settings: &Settings) {
    let mut client = postgres::Client::connect(conn_str, postgres::NoTls).unwrap();
    client.batch_execute("drop table if exists good;").unwrap();

    let mut config = Config::with_settings(settings);
    config
        .use_migrations(&[EmbeddedMigration::with_tag("bad")
            .up("create table good (x integer); insert into does_not_exist values (1);")
            .down("drop table good;")
            .boxed()])
        .unwrap();
    config.setup().unwrap();
    let config = config.reload().unwrap();

    let res = Migrator::with_config(&config).show_output(false).apply();
    assert!(res.is_err(), "a migration with invalid sql must fail");

    let good_exists: bool = client
        .query_one(
            "select exists(select 1 from pg_tables where tablename = 'good')",
            &[],
        )
        .unwrap()
        .get(0);
    assert!(!good_exists, "partial DDL must be rolled back");

    let config = config.reload().unwrap();
    let statuses = migrant_lib::migration_statuses(&config).unwrap();
    assert!(
        statuses.iter().all(|m| !m.applied),
        "the tag must not be recorded when the migration fails"
    );

    client.batch_execute("drop table if exists good;").unwrap();
}

/// A `force`d run continues past a failed migration and applies the rest on the
/// same locked session (the connection is recovered in place on the error, so
/// the advisory lock is never released mid-run). Shares the postgres database
/// with `postgres_end_to_end`, so it runs as one of its phases.
#[cfg(feature = "d-postgres")]
fn assert_force_continues_holding_lock(conn_str: &str, settings: &Settings) {
    let mut client = postgres::Client::connect(conn_str, postgres::NoTls).unwrap();
    client.batch_execute("drop table if exists later;").unwrap();

    let mut config = Config::with_settings(settings);
    config
        .use_migrations(&[
            EmbeddedMigration::with_tag("bad")
                .up("insert into does_not_exist values (1);")
                .down("select 1;")
                .boxed(),
            EmbeddedMigration::with_tag("later")
                .up("create table later (x integer);")
                .down("drop table later;")
                .boxed(),
        ])
        .unwrap();
    config.setup().unwrap();
    let config = config.reload().unwrap();

    // force past the failing first migration; the run must continue and apply
    // the second on the same session that still holds the advisory lock.
    Migrator::with_config(&config)
        .all(true)
        .force(true)
        .show_output(false)
        .swallow_completion(true)
        .apply()
        .unwrap();

    let later_exists: bool = client
        .query_one(
            "select exists(select 1 from pg_tables where tablename = 'later')",
            &[],
        )
        .unwrap()
        .get(0);
    assert!(
        later_exists,
        "force must continue past the failure and apply later migrations"
    );

    let config = config.reload().unwrap();
    let statuses = migrant_lib::migration_statuses(&config).unwrap();
    assert!(
        statuses.iter().all(|m| m.applied),
        "force records every migration as applied, including the failed one"
    );

    client.batch_execute("drop table if exists later;").unwrap();
}

#[cfg(feature = "d-mysql")]
#[test]
fn mysql_end_to_end() {
    let conn_str = match std::env::var("MYSQL_TEST_CONN_STR") {
        Ok(s) => s,
        Err(_) => {
            eprintln!("MYSQL_TEST_CONN_STR not set, skipping");
            return;
        }
    };
    let parts = parse_conn_str(&conn_str, 3306);
    let settings = Settings::configure_mysql()
        .database_name(&parts.name)
        .database_user(&parts.user)
        .database_password(&parts.password)
        .database_host(&parts.host)
        .database_port(parts.port)
        .build()
        .unwrap();
    // drop any leftover table from an earlier interrupted run
    drop_mysql_migration_table(&conn_str);
    apply_and_unapply(&settings);
    drop_mysql_migration_table(&conn_str);
}
