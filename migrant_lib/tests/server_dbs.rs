//! End-to-end tests against server databases (postgres, mysql).
//!
//! These are skipped unless the corresponding connection string is provided:
//! `POSTGRES_TEST_CONN_STR` (e.g. `postgres://user:pass@localhost:5432/db`)
//! and `MYSQL_TEST_CONN_STR` (e.g. `mysql://user:pass@localhost:3306/db`).
#![cfg(any(feature = "postgres", feature = "mysql"))]

use migrant_lib::{Config, Direction, EmbeddedMigration, ForceMode, Migrator, Settings};

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
    let mut config = Config::with_settings(settings.clone());
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
    assert!(statuses.iter().all(|m| m.applied()));

    Migrator::with_config(&config)
        .direction(Direction::Down)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();

    let config = config.reload().unwrap();
    let statuses = migrant_lib::migration_statuses(&config).unwrap();
    assert!(statuses.iter().all(|m| !m.applied()));
}

/// Drop the migration table so the next run starts from a clean database.
#[cfg(feature = "postgres")]
fn drop_pg_migration_table(conn_str: &str) {
    let mut client = postgres::Client::connect(conn_str, postgres::NoTls)
        .expect("connect to drop postgres migration table");
    client
        .batch_execute("drop table if exists __migrant_migrations;")
        .expect("drop postgres migration table");
}

/// Drop the migration table so the next run starts from a clean database.
#[cfg(feature = "mysql")]
fn drop_mysql_migration_table(conn_str: &str) {
    use mysql::prelude::Queryable;
    let opts = mysql::Opts::from_url(conn_str).expect("parse mysql connection string");
    let mut conn = mysql::Conn::new(opts).expect("connect to drop mysql migration table");
    conn.query_drop("drop table if exists __migrant_migrations;")
        .expect("drop mysql migration table");
}

#[cfg(feature = "postgres")]
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
    // schema (checksum/applied_at) + recorded-order phase, same database
    assert_pg_schema_records_checksum_and_order(&conn_str, &settings);
    drop_pg_migration_table(&conn_str);
    // atomic-rollback phase runs against the same database (see the helper doc)
    assert_failed_migration_rolls_back(&conn_str, &settings);
    drop_pg_migration_table(&conn_str);
    // force-past-failure phase, also against the same database
    assert_force_continues_holding_lock(&conn_str, &settings);
    drop_pg_migration_table(&conn_str);
    // synchronized(false) phase, also against the same database
    assert_unsynchronized_run_skips_lock(&conn_str, &settings);
    drop_pg_migration_table(&conn_str);
}

/// With `synchronized(false)` a run must not take the migration advisory lock:
/// it completes even while another session holds that lock. Shares the
/// postgres database with `postgres_end_to_end`, so it runs as one of its
/// phases.
#[cfg(feature = "postgres")]
fn assert_unsynchronized_run_skips_lock(conn_str: &str, settings: &Settings) {
    // Must match `ADVISORY_LOCK_KEY` in `src/drivers/pg.rs`.
    const ADVISORY_LOCK_KEY: i64 = 30_796_665_483_397_364;

    let mut client = postgres::Client::connect(conn_str, postgres::NoTls).unwrap();
    let got: bool = client
        .query_one("select pg_try_advisory_lock($1)", &[&ADVISORY_LOCK_KEY])
        .unwrap()
        .get(0);
    assert!(got, "test precondition: advisory lock must be acquirable");

    // Run the migrator on another thread so a regression (taking the lock and
    // blocking on it forever) fails the test instead of hanging it.
    let (tx, rx) = std::sync::mpsc::channel();
    let settings = settings.clone();
    std::thread::spawn(move || {
        let run = || -> Result<(), migrant_lib::Error> {
            let mut config = Config::with_settings(settings);
            config.use_migrations(&[EmbeddedMigration::with_tag("unsync")
                .up("select 1;")
                .down("select 1;")
                .boxed()])?;
            config.setup()?;
            Migrator::with_config(&config)
                .synchronized(false)
                .show_output(false)
                .apply()?;
            Ok(())
        };
        tx.send(run()).expect("send unsynchronized run result");
    });
    let res = rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("unsynchronized run must not block on the held advisory lock");
    res.expect("unsynchronized run must apply cleanly");

    client
        .execute("select pg_advisory_unlock_all()", &[])
        .unwrap();
}

/// After applying two embedded migrations, the `__migrant_migrations` table on
/// postgres carries the new bookkeeping columns: `applied_at` is populated by
/// the column default, `checksum` holds the sha256 of each migration's up SQL,
/// and `order by id` reflects the recorded application order. Shares the
/// postgres database with `postgres_end_to_end`, so it runs as one of its phases.
#[cfg(feature = "postgres")]
fn assert_pg_schema_records_checksum_and_order(conn_str: &str, settings: &Settings) {
    use sha2::{Digest, Sha256};

    fn sha256_hex(bytes: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(bytes);
        h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
    }

    let up_a = "create table users (name varchar(64));";
    let up_b = "insert into users (name) values ('james');";

    let mut config = Config::with_settings(settings.clone());
    config
        .use_migrations(&[
            EmbeddedMigration::with_tag("create-users")
                .up(up_a)
                .down("drop table users;")
                .boxed(),
            EmbeddedMigration::with_tag("seed-users")
                .up(up_b)
                .down("delete from users where name = 'james';")
                .boxed(),
        ])
        .unwrap();
    config.setup().unwrap();
    let config = config.reload().unwrap();
    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();

    let mut client = postgres::Client::connect(conn_str, postgres::NoTls).unwrap();

    // The new columns exist.
    for col in ["id", "tag", "checksum", "applied_at"] {
        let exists: bool = client
            .query_one(
                "select exists(select 1 from information_schema.columns \
                 where table_name = '__migrant_migrations' and column_name = $1)",
                &[&col],
            )
            .unwrap()
            .get(0);
        assert!(
            exists,
            "column `{}` must exist on __migrant_migrations",
            col
        );
    }

    // Recorded order (order by id) matches application order, and checksums are
    // the sha256 of each up SQL.
    let rows = client
        .query(
            "select tag, checksum, applied_at is not null \
             from __migrant_migrations order by id",
            &[],
        )
        .unwrap();
    let recorded: Vec<(String, Option<String>, bool)> = rows
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect();
    assert_eq!(
        vec![
            (
                "create-users".to_string(),
                Some(sha256_hex(up_a.as_bytes())),
                true
            ),
            (
                "seed-users".to_string(),
                Some(sha256_hex(up_b.as_bytes())),
                true
            ),
        ],
        recorded,
    );

    Migrator::with_config(&config)
        .direction(Direction::Down)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
}

/// A migration whose SQL fails partway is rolled back atomically on postgres:
/// the partial DDL is undone and the bookkeeping row is never written.
///
/// Not a standalone `#[test]`: it shares the one postgres database (and the
/// single `__migrant_migrations` table) with `postgres_end_to_end`, so it runs
/// as a phase of that test rather than racing it under cargo's parallel runner.
#[cfg(feature = "postgres")]
fn assert_failed_migration_rolls_back(conn_str: &str, settings: &Settings) {
    let mut client = postgres::Client::connect(conn_str, postgres::NoTls).unwrap();
    client.batch_execute("drop table if exists good;").unwrap();

    let mut config = Config::with_settings(settings.clone());
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
        statuses.iter().all(|m| !m.applied()),
        "the tag must not be recorded when the migration fails"
    );

    client.batch_execute("drop table if exists good;").unwrap();
}

/// A `force`d run continues past a failed migration and applies the rest on the
/// same locked session (the connection is recovered in place on the error, so
/// the advisory lock is never released mid-run). Shares the postgres database
/// with `postgres_end_to_end`, so it runs as one of its phases.
#[cfg(feature = "postgres")]
fn assert_force_continues_holding_lock(conn_str: &str, settings: &Settings) {
    let mut client = postgres::Client::connect(conn_str, postgres::NoTls).unwrap();
    client.batch_execute("drop table if exists later;").unwrap();

    let mut config = Config::with_settings(settings.clone());
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
        .force(ForceMode::AcceptFailures)
        .show_output(false)
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
        statuses.iter().all(|m| m.applied()),
        "force records every migration as applied, including the failed one"
    );

    client.batch_execute("drop table if exists later;").unwrap();
}

#[cfg(feature = "mysql")]
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
    // schema (checksum/applied_at) + recorded-order phase, same database
    assert_mysql_schema_records_checksum_and_order(&conn_str, &settings);
    drop_mysql_migration_table(&conn_str);
}

/// After applying two embedded migrations, the `__migrant_migrations` table on
/// mysql carries the new bookkeeping columns: `applied_at` is populated by the
/// column default, `checksum` holds the sha256 of each migration's up SQL, and
/// `order by id` reflects the recorded application order. Shares the mysql
/// database with `mysql_end_to_end`, so it runs as one of its phases.
#[cfg(feature = "mysql")]
fn assert_mysql_schema_records_checksum_and_order(conn_str: &str, settings: &Settings) {
    use mysql::prelude::Queryable;
    use sha2::{Digest, Sha256};

    fn sha256_hex(bytes: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(bytes);
        h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
    }

    let up_a = "create table users (name varchar(64));";
    let up_b = "insert into users (name) values ('james');";

    let mut config = Config::with_settings(settings.clone());
    config
        .use_migrations(&[
            EmbeddedMigration::with_tag("create-users")
                .up(up_a)
                .down("drop table users;")
                .boxed(),
            EmbeddedMigration::with_tag("seed-users")
                .up(up_b)
                .down("delete from users where name = 'james';")
                .boxed(),
        ])
        .unwrap();
    config.setup().unwrap();
    let config = config.reload().unwrap();
    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();

    let opts = mysql::Opts::from_url(conn_str).unwrap();
    let mut conn = mysql::Conn::new(opts).unwrap();

    // The new columns exist.
    for col in ["id", "tag", "checksum", "applied_at"] {
        let exists: Option<i64> = conn
            .exec_first(
                "select count(*) from information_schema.columns \
                 where table_name = '__migrant_migrations' \
                 and table_schema = database() and column_name = ?",
                (col,),
            )
            .unwrap();
        assert_eq!(Some(1), exists, "column `{}` must exist", col);
    }

    // Recorded order (order by id) matches application order, and checksums are
    // the sha256 of each up SQL.
    let recorded: Vec<(String, Option<String>, i64)> = conn
        .query(
            "select tag, checksum, (applied_at is not null) \
             from __migrant_migrations order by id",
        )
        .unwrap();
    assert_eq!(
        vec![
            (
                "create-users".to_string(),
                Some(sha256_hex(up_a.as_bytes())),
                1
            ),
            (
                "seed-users".to_string(),
                Some(sha256_hex(up_b.as_bytes())),
                1
            ),
        ],
        recorded,
    );

    Migrator::with_config(&config)
        .direction(Direction::Down)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
}
