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

fn apply_and_unapply(settings: Settings) {
    let mut config = Config::with_settings(&settings);
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
    apply_and_unapply(settings);
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
    apply_and_unapply(settings);
}
