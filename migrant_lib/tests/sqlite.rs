//! End-to-end sqlite tests, covering in-memory databases where the
//! connection handle must be kept alive across all operations.
#![cfg(feature = "sqlite")]

use migrant_lib::{
    Config, ConnConfig, Direction, EmbeddedMigration, FileMigration, FnMigration, ForceMode,
    Migratable, Migrator, Settings,
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
    let mut config = Config::with_settings(settings.clone());
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
        .filter(|m| m.applied())
        .map(|m| m.tag().to_string())
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

/// Read the `(tag, checksum)` bookkeeping rows in recorded (`order by id`) order.
fn recorded_rows(config: &Config) -> Vec<(String, Option<String>)> {
    let handle = config.sqlite_connection().unwrap();
    let conn = handle.lock().unwrap();
    let mut stmt = conn
        .prepare("select tag, checksum from __migrant_migrations order by id")
        .unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })
        .unwrap();
    rows.collect::<Result<Vec<_>, _>>().unwrap()
}

/// Insert a raw bookkeeping tag directly (no checksum), simulating a tag the
/// database records that the running code does not manage.
fn raw_insert_tag(config: &Config, tag: &str) {
    let handle = config.sqlite_connection().unwrap();
    let conn = handle.lock().unwrap();
    conn.execute("insert into __migrant_migrations (tag) values (?1)", [tag])
        .unwrap();
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64
        && s.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// Build an in-memory config with a single migration whose `up` creates a table
/// and then runs an invalid statement, so application fails partway through.
fn failing_migration_config(no_transaction: bool) -> Config {
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let mut migration = EmbeddedMigration::with_tag("bad")
        .up("create table good (x integer); insert into does_not_exist values (1);")
        .down("drop table good;");
    if no_transaction {
        migration = migration.no_transaction();
    }
    let mut config = Config::with_settings(settings);
    config.use_migrations(&[migration.boxed()]).unwrap();
    config
}

/// Build an in-memory config where the first migration fails and a second,
/// valid migration follows it -- the shape `force` runs care about.
fn failing_then_good_config() -> Config {
    let settings = Settings::configure_sqlite().memory().build().unwrap();
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
    config
}

#[test]
fn force_accept_failures_records_failed_migration() {
    let config = failing_then_good_config();
    config.setup().unwrap();

    Migrator::with_config(&config)
        .all(true)
        .force(ForceMode::AcceptFailures)
        .show_output(false)
        .apply()
        .unwrap();

    let config = config.reload().unwrap();
    assert!(
        table_exists(&config, "later"),
        "the run must continue past the failure and apply later migrations"
    );
    assert_eq!(
        vec!["bad".to_string(), "later".to_string()],
        applied_tags(&config),
        "accept-failures records the failed migration as applied"
    );
}

#[test]
fn force_skip_failures_leaves_failed_migration_unrecorded() {
    let config = failing_then_good_config();
    config.setup().unwrap();

    Migrator::with_config(&config)
        .all(true)
        .force(ForceMode::SkipFailures)
        .show_output(false)
        .apply()
        .unwrap();

    let config = config.reload().unwrap();
    assert!(
        table_exists(&config, "later"),
        "the run must continue past the failure and apply later migrations"
    );
    assert_eq!(
        vec!["later".to_string()],
        applied_tags(&config),
        "skip-failures must not record the failed migration"
    );

    // The skipped `bad` migration was left unrecorded, so `later` is now applied
    // ahead of the still-pending `bad`. Under the strict default a following run
    // surfaces that gap as an ordering error rather than silently proceeding.
    let err = Migrator::with_config(&config)
        .show_output(false)
        .apply()
        .unwrap_err();
    assert!(
        err.is_migration_ordering(),
        "the unrecorded skipped migration leaves an out-of-order gap on the next run, got: {err:?}"
    );

    // Opting out of the ordering check retries the skipped `bad`, which fails
    // again without force.
    let res = Migrator::with_config(&config)
        .allow_out_of_order(true)
        .show_output(false)
        .apply();
    assert!(
        res.is_err(),
        "the skipped migration must be selected and fail again on retry"
    );
}

#[test]
fn apply_refreshes_applied_state_without_manual_reload() {
    // Consumers are not required to call `Config::reload` before applying:
    // the migrator re-reads applied state itself, so back-to-back runs on the
    // same un-reloaded config must not re-apply migration 1.
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let config = migrations_config(&settings);
    config.setup().unwrap();

    // First single apply: create-users.
    Migrator::with_config(&config)
        .show_output(false)
        .apply()
        .unwrap();

    // Second single apply on the same, never-reloaded config: must pick
    // seed-users, not fail re-running create-users.
    Migrator::with_config(&config)
        .show_output(false)
        .apply()
        .unwrap();

    let config = config.reload().unwrap();
    assert_eq!(
        vec!["create-users".to_string(), "seed-users".to_string()],
        applied_tags(&config)
    );
    assert_eq!(1, user_count(&config));
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
fn embedded_directive_opts_up_out_of_transaction() {
    // `up` carries the `-- migrant:no-transaction` directive and fails partway.
    // Without a wrapping transaction the earlier `create table` persists,
    // proving the directive was read from the embedded up SQL.
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let mut config = Config::with_settings(settings);
    config
        .use_migrations(&[EmbeddedMigration::with_tag("bad-up")
            .up("-- migrant:no-transaction\ncreate table up_good (x integer); insert into nope values (1);")
            .down("select 1;")
            .boxed()])
        .unwrap();
    config.setup().unwrap();
    let config = config.reload().unwrap();

    let res = Migrator::with_config(&config).show_output(false).apply();
    assert!(res.is_err(), "a migration with invalid sql must fail");

    let config = config.reload().unwrap();
    assert!(
        table_exists(&config, "up_good"),
        "the directive up must run without a transaction, leaving the partial create"
    );
    assert!(applied_tags(&config).is_empty());
}

#[test]
fn directive_applies_per_direction() {
    // `up` has no directive (transactional); `down` carries the directive
    // (non-transactional). Applying up succeeds; a failing down then leaves its
    // partial state behind, demonstrating the flag is resolved per direction.
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let mut config = Config::with_settings(settings);
    config
        .use_migrations(&[EmbeddedMigration::with_tag("thing")
            .up("create table thing (x integer);")
            .down("-- migrant:no-transaction\ncreate table down_good (x integer); insert into nope values (1);")
            .boxed()])
        .unwrap();
    config.setup().unwrap();
    let config = config.reload().unwrap();

    Migrator::with_config(&config)
        .show_output(false)
        .apply()
        .unwrap();
    let config = config.reload().unwrap();
    assert_eq!(vec!["thing".to_string()], applied_tags(&config));

    let res = Migrator::with_config(&config)
        .direction(Direction::Down)
        .show_output(false)
        .apply();
    assert!(res.is_err(), "the failing down migration must error");

    let config = config.reload().unwrap();
    assert!(
        table_exists(&config, "down_good"),
        "the directive down must run without a transaction, leaving the partial create"
    );
}

#[test]
fn file_migration_reads_no_transaction_directive() {
    // The `migrant` CLI discovers file migrations, so the directive must be read
    // from the up.sql on disk (not only from an in-code builder call).
    let dir = tempfile::tempdir().unwrap();
    let up = dir.path().join("up.sql");
    let down = dir.path().join("down.sql");
    std::fs::write(
        &up,
        "-- migrant:no-transaction\ncreate table up_good (x integer); insert into nope values (1);",
    )
    .unwrap();
    std::fs::write(&down, "select 1;").unwrap();

    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let mut config = Config::with_settings(settings);
    config
        .use_migrations(&[FileMigration::with_tag("filed").up(&up).down(&down).boxed()])
        .unwrap();
    config.setup().unwrap();
    let config = config.reload().unwrap();

    let res = Migrator::with_config(&config).show_output(false).apply();
    assert!(res.is_err(), "a migration with invalid sql must fail");

    let config = config.reload().unwrap();
    assert!(
        table_exists(&config, "up_good"),
        "the file directive must opt the up out of a transaction"
    );
    assert!(applied_tags(&config).is_empty());
}

#[test]
fn file_database_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let settings = Settings::configure_sqlite()
        .database_path(&db_path)
        .build()
        .unwrap();
    let config = migrations_config(&settings);
    config.setup().unwrap();
    assert!(db_path.exists(), "setup creates the database file");

    let config = config.reload().unwrap();
    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();

    let config = config.reload().unwrap();
    assert_eq!(2, applied_tags(&config).len());
    assert_eq!(1, user_count(&config));
}

#[test]
fn checksum_recorded_for_embedded_null_for_fn_and_ordered_by_id() {
    // `create-users` is an embedded (SQL) migration and gets a checksum;
    // `seed-users` is a function migration and stores NULL by design. The rows
    // come back in recorded application order via `order by id`.
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let config = migrations_config(&settings);
    config.setup().unwrap();
    let config = config.reload().unwrap();

    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();

    let rows = recorded_rows(&config);
    assert_eq!(2, rows.len());
    assert_eq!("create-users", rows[0].0);
    assert_eq!("seed-users", rows[1].0);
    let embedded_sum = rows[0]
        .1
        .as_deref()
        .expect("embedded migration has a checksum");
    assert!(
        is_hex64(embedded_sum),
        "checksum must be 64 hex chars: {embedded_sum}"
    );
    assert_eq!(None, rows[1].1, "function migrations store NULL checksum");
}

#[test]
fn file_migration_checksum_is_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let up = dir.path().join("up.sql");
    let down = dir.path().join("down.sql");
    std::fs::write(&up, "create table filed (x integer);").unwrap();
    std::fs::write(&down, "drop table filed;").unwrap();

    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let mut config = Config::with_settings(settings);
    config
        .use_migrations(&[FileMigration::with_tag("filed").up(&up).down(&down).boxed()])
        .unwrap();
    config.setup().unwrap();
    let config = config.reload().unwrap();

    Migrator::with_config(&config)
        .show_output(false)
        .apply()
        .unwrap();

    let rows = recorded_rows(&config);
    assert_eq!(1, rows.len());
    let sum = rows[0].1.as_deref().expect("file migration has a checksum");
    assert!(is_hex64(sum), "checksum must be 64 hex chars: {sum}");
}

#[test]
fn down_reverts_last_applied_by_recorded_order() {
    // Recorded order is authoritative: a single Down after a full Up reverts the
    // most recently applied migration (`seed-users`), leaving the earlier one.
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let config = migrations_config(&settings);
    config.setup().unwrap();
    let config = config.reload().unwrap();

    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();

    Migrator::with_config(&config)
        .direction(Direction::Down)
        .show_output(false)
        .apply()
        .unwrap();

    let config = config.reload().unwrap();
    assert_eq!(vec!["create-users".to_string()], applied_tags(&config));
}

#[test]
fn unknown_applied_tag_errors_and_allow_unknown_tags_opts_out() {
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let config = migrations_config(&settings);
    config.setup().unwrap();
    let config = config.reload().unwrap();

    // Apply the first migration, then record a tag the code does not manage.
    Migrator::with_config(&config)
        .show_output(false)
        .apply()
        .unwrap();
    raw_insert_tag(&config, "ghost");

    // A default Up run aborts on the unknown applied tag.
    let err = Migrator::with_config(&config)
        .show_output(false)
        .apply()
        .unwrap_err();
    assert!(
        err.is_migration_not_found(),
        "unknown applied tag must raise MigrationNotFound, got: {err:?}"
    );

    // Opting out lets the run ignore `ghost` and apply the remaining migration.
    Migrator::with_config(&config)
        .allow_unknown_tags(true)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
    let config = config.reload().unwrap();
    let applied = applied_tags(&config);
    assert!(applied.contains(&"create-users".to_string()));
    assert!(applied.contains(&"seed-users".to_string()));
}

#[test]
fn same_second_file_migrations_apply_deterministically_under_strict_checks() {
    // Regression: file migrations sharing a timestamp second must have a total,
    // deterministic discovery order. The migrator re-runs discovery on every
    // step and treats the order as authoritative; without a stamp-tie tiebreak
    // the random HashMap order could differ between steps and the default strict
    // ordering check would spuriously abort a legitimate `apply` of same-second
    // migrations. A full apply must succeed and leave every migration applied.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // Four migrations, all the same second, folder-creation order deliberately
    // unsorted relative to the resolved (tag-sorted) order.
    for (folder, tbl) in [
        ("20200101000000_delta", "t_delta"),
        ("20200101000000_alpha", "t_alpha"),
        ("20200101000000_charlie", "t_charlie"),
        ("20200101000000_bravo", "t_bravo"),
    ] {
        let d = root.join(folder);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("up.sql"), format!("create table {tbl} (x integer);")).unwrap();
        std::fs::write(d.join("down.sql"), format!("drop table {tbl};")).unwrap();
    }

    // In-memory database, migrations discovered from the on-disk location (so
    // `available_migrations` goes through `search_for_migrations`, not explicit
    // `use_migrations`).
    let settings = Settings::configure_sqlite()
        .memory()
        .migration_location(root)
        .build()
        .unwrap();
    let config = Config::with_settings(settings);
    config.setup().unwrap();
    let config = config.reload().unwrap();

    // Must not raise `MigrationOrdering` under the default strict checks.
    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .expect("same-second migrations must apply cleanly under strict ordering");

    let config = config.reload().unwrap();
    assert_eq!(
        vec![
            "20200101000000_alpha".to_string(),
            "20200101000000_bravo".to_string(),
            "20200101000000_charlie".to_string(),
            "20200101000000_delta".to_string(),
        ],
        applied_tags(&config),
        "applied in the deterministic tag-sorted order"
    );
    for tbl in ["t_alpha", "t_bravo", "t_charlie", "t_delta"] {
        assert!(table_exists(&config, tbl), "{tbl} must have been created");
    }
}

#[test]
fn out_of_order_applied_tag_errors_and_allow_out_of_order_opts_out() {
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let mut config = Config::with_settings(settings);
    config
        .use_migrations(&[
            EmbeddedMigration::with_tag("a")
                .up("create table a (x integer);")
                .down("drop table a;")
                .boxed(),
            EmbeddedMigration::with_tag("b")
                .up("create table b (x integer);")
                .down("drop table b;")
                .boxed(),
            EmbeddedMigration::with_tag("c")
                .up("create table c (x integer);")
                .down("drop table c;")
                .boxed(),
        ])
        .unwrap();
    config.setup().unwrap();
    let config = config.reload().unwrap();

    // Apply `a`, then record `c` as applied while `b` is still pending: `c` is
    // out of order relative to definition order.
    Migrator::with_config(&config)
        .show_output(false)
        .apply()
        .unwrap();
    raw_insert_tag(&config, "c");

    let err = Migrator::with_config(&config)
        .show_output(false)
        .apply()
        .unwrap_err();
    assert!(
        err.is_migration_ordering(),
        "out-of-order application must raise MigrationOrdering, got: {err:?}"
    );

    // Opting out applies the intervening `b`.
    Migrator::with_config(&config)
        .allow_out_of_order(true)
        .show_output(false)
        .apply()
        .unwrap();
    assert!(
        table_exists(&config, "b"),
        "the intervening migration must run"
    );
}

/// A `FileMigration` whose `up` file does not exist must fail to apply and record
/// *nothing* -- it must not silently record a NULL checksum (which `checksum()`
/// returns for an unreadable file) and report success. This guards the divergence
/// between the `checksum()` read (best-effort `None`) and the apply-path read
/// (which surfaces the real error).
#[test]
fn file_migration_missing_up_file_fails_and_records_nothing() {
    let dir = tempfile::tempdir().unwrap();
    // `down` exists but `up` points at a file that was never created.
    let down = dir.path().join("down.sql");
    std::fs::write(&down, "select 1;").unwrap();
    let missing_up = dir.path().join("does_not_exist_up.sql");

    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let mut config = Config::with_settings(settings);
    config
        .use_migrations(&[FileMigration::with_tag("filed")
            .up(&missing_up)
            .down(&down)
            .boxed()])
        .unwrap();
    config.setup().unwrap();
    let config = config.reload().unwrap();

    let res = Migrator::with_config(&config).show_output(false).apply();
    assert!(
        res.is_err(),
        "applying a migration whose up file is missing must error, not silently succeed"
    );

    let config = config.reload().unwrap();
    assert!(
        applied_tags(&config).is_empty(),
        "an unreadable up file must not record the tag"
    );
    assert!(
        recorded_rows(&config).is_empty(),
        "no bookkeeping row (and so no NULL-checksum row) may be written for a failed read"
    );
}

/// Recorded application order (`order by id`) is authoritative even when a
/// migration is applied out of definition order: the intervening migration gets
/// the *latest* id, so `order by id` lists it last, and a subsequent default
/// `Down` reverts it first (the most recently applied by recorded order).
#[test]
fn recorded_order_reflects_out_of_order_run_and_down_reverts_last_applied() {
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let mut config = Config::with_settings(settings);
    config
        .use_migrations(&[
            EmbeddedMigration::with_tag("a")
                .up("create table a (x integer);")
                .down("drop table a;")
                .boxed(),
            EmbeddedMigration::with_tag("b")
                .up("create table b (x integer);")
                .down("drop table b;")
                .boxed(),
            EmbeddedMigration::with_tag("c")
                .up("create table c (x integer);")
                .down("drop table c;")
                .boxed(),
        ])
        .unwrap();
    config.setup().unwrap();
    let config = config.reload().unwrap();

    // Apply `a`, then record `c` (as if it ran on another branch), then apply the
    // intervening `b` with the out-of-order opt-out. `b` is inserted last.
    Migrator::with_config(&config)
        .show_output(false)
        .apply()
        .unwrap();
    raw_insert_tag(&config, "c");
    let config = config.reload().unwrap();
    Migrator::with_config(&config)
        .allow_out_of_order(true)
        .show_output(false)
        .apply()
        .unwrap();

    let config = config.reload().unwrap();
    // `order by id` reflects the true application order: a, c, then b (applied
    // last), *not* definition order a, b, c.
    let order: Vec<String> = recorded_rows(&config)
        .into_iter()
        .map(|(tag, _)| tag)
        .collect();
    assert_eq!(
        vec!["a".to_string(), "c".to_string(), "b".to_string()],
        order
    );

    // A default Down reverts the most recently applied by recorded order: `b`.
    Migrator::with_config(&config)
        .direction(Direction::Down)
        .show_output(false)
        .apply()
        .unwrap();
    let config = config.reload().unwrap();
    let remaining: Vec<String> = recorded_rows(&config)
        .into_iter()
        .map(|(tag, _)| tag)
        .collect();
    assert_eq!(vec!["a".to_string(), "c".to_string()], remaining);
    assert!(!table_exists(&config, "b"), "down must have reverted `b`");
}

/// A user-defined `Migratable` (only `'static + Clone`, using the trait's default
/// `checksum`) must be usable end to end: the sealed `MigratableClone` blanket
/// impl clones it into the boxed set, it applies, and -- inheriting the default
/// `checksum` of `None` -- records a NULL checksum.
#[test]
fn custom_migratable_applies_and_records_null_checksum() {
    #[derive(Clone)]
    struct Custom {
        tag: String,
    }
    impl Migratable for Custom {
        fn apply_up(&self, config: &Config) -> Result<(), Box<dyn std::error::Error>> {
            let handle = config.sqlite_connection()?;
            let conn = handle.lock().unwrap();
            conn.execute_batch("create table custom_made (x integer);")?;
            Ok(())
        }
        fn apply_down(&self, config: &Config) -> Result<(), Box<dyn std::error::Error>> {
            let handle = config.sqlite_connection()?;
            let conn = handle.lock().unwrap();
            conn.execute_batch("drop table custom_made;")?;
            Ok(())
        }
        fn tag(&self) -> String {
            self.tag.clone()
        }
    }

    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let mut config = Config::with_settings(settings);
    config
        .use_migrations(&[Box::new(Custom {
            tag: "custom".to_string(),
        }) as Box<dyn Migratable>])
        .unwrap();
    config.setup().unwrap();
    let config = config.reload().unwrap();

    Migrator::with_config(&config)
        .show_output(false)
        .apply()
        .unwrap();

    let config = config.reload().unwrap();
    assert!(
        table_exists(&config, "custom_made"),
        "the custom migration's up must have run"
    );
    let rows = recorded_rows(&config);
    assert_eq!(1, rows.len());
    assert_eq!("custom", rows[0].0);
    assert_eq!(
        None, rows[0].1,
        "a custom migration inherits the default `checksum` of None (NULL)"
    );
}

#[test]
fn checksum_drift_aborts_up_run_and_opt_out_applies() {
    // Apply a migration, then swap in a same-tag migration whose up-SQL differs
    // (as if the file had been edited after it ran). The recorded checksum no
    // longer matches the current one, so the next up run aborts before applying.
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let mut config = Config::with_settings(settings);
    config
        .use_migrations(&[EmbeddedMigration::with_tag("t")
            .up("create table t (x integer);")
            .down("drop table t;")
            .boxed()])
        .unwrap();
    config.setup().unwrap();
    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
    let recorded = recorded_rows(&config)[0].1.clone();
    assert!(
        recorded.is_some(),
        "the up-SQL migration records a checksum"
    );

    // Same tag, edited up-SQL: a different checksum, on the same connection.
    config
        .use_migrations(&[EmbeddedMigration::with_tag("t")
            .up("create table t (x integer, y integer);")
            .down("drop table t;")
            .boxed()])
        .unwrap();

    let err = Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap_err();
    assert!(
        err.is_checksum_mismatch(),
        "an edited already-applied migration must abort the run, got: {err:?}"
    );
    assert!(
        format!("{err}").contains("t"),
        "the error should name the drifted tag: {err}"
    );

    // The recorded checksum is unchanged: the aborted run recorded nothing.
    assert_eq!(
        recorded,
        recorded_rows(&config)[0].1,
        "an aborted drift run must not rewrite the recorded checksum"
    );

    // Opting out bypasses the check. The migration is already applied, so the
    // run has nothing to do and simply reports empty instead of erroring.
    let report = Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .allow_checksum_mismatch(true)
        .apply()
        .unwrap();
    assert!(
        report.is_empty(),
        "opting out applies despite drift; nothing was pending here"
    );
}

#[test]
fn matching_checksum_does_not_trip_drift_check() {
    // Re-running with the identical migration set must not be flagged as drift.
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let mut config = Config::with_settings(settings);
    config
        .use_migrations(&[EmbeddedMigration::with_tag("t")
            .up("create table t (x integer);")
            .down("drop table t;")
            .boxed()])
        .unwrap();
    config.setup().unwrap();
    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
    // A second up run over the unchanged set validates checksums and finds no
    // drift, returning an empty report rather than erroring.
    let report = Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
    assert!(report.is_empty(), "unchanged checksums must not error");
}

#[test]
fn null_recorded_checksum_is_not_treated_as_drift() {
    // A programmatic migration records a NULL checksum. Re-running after it is
    // applied must not flag drift even though the migration reports no checksum
    // to compare against.
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let config = migrations_config(&settings);
    config.setup().unwrap();
    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
    // `seed-users` is an `FnMigration` recorded with a NULL checksum.
    let rows = recorded_rows(&config);
    assert!(
        rows.iter()
            .any(|(tag, sum)| tag == "seed-users" && sum.is_none()),
        "the programmatic migration records a NULL checksum: {rows:?}"
    );
    let report = Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
    assert!(
        report.is_empty(),
        "a NULL recorded checksum is skipped, not treated as drift"
    );
}

/// Read the full bookkeeping rows in recorded (`order by id`) order, including
/// each row's id and repeatable flag.
fn recorded_repeatable_rows(config: &Config) -> Vec<(i64, String, Option<String>, bool)> {
    let handle = config.sqlite_connection().unwrap();
    let conn = handle.lock().unwrap();
    let mut stmt = conn
        .prepare("select id, tag, checksum, is_repeatable from __migrant_migrations order by id")
        .unwrap();
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap();
    rows.collect::<Result<Vec<_>, _>>().unwrap()
}

/// An in-memory config with one versioned migration creating a `roles` table
/// and one repeatable migration seeding it with the given SQL.
fn repeatable_config(settings: &Settings, seed_sql: &'static str) -> Config {
    let mut config = Config::with_settings(settings.clone());
    config
        .use_migrations(&[
            EmbeddedMigration::with_tag("create-roles")
                .up("create table roles (name text);")
                .down("drop table roles;")
                .boxed(),
            EmbeddedMigration::with_tag("seed-roles")
                .repeatable()
                .up(seed_sql)
                .boxed(),
        ])
        .unwrap();
    config
}

fn role_names(config: &Config) -> Vec<String> {
    let handle = config.sqlite_connection().unwrap();
    let conn = handle.lock().unwrap();
    let mut stmt = conn
        .prepare("select name from roles order by name")
        .unwrap();
    let rows = stmt.query_map([], |r| r.get(0)).unwrap();
    rows.collect::<Result<Vec<String>, _>>().unwrap()
}

// REPEAT-1, REPEAT-5, REPEAT-6
#[test]
fn repeatable_migration_reruns_only_when_its_checksum_changes() {
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let config = repeatable_config(&settings, "insert into roles (name) values ('admin');");
    config.setup().unwrap();

    // First run: the versioned migration, then the repeatable one.
    let report = Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
    assert_eq!(report.tags(), ["create-roles", "seed-roles"]);
    assert_eq!(report.repeatable_tags(), ["seed-roles"]);
    assert_eq!(role_names(&config), ["admin"]);

    // Second run over unchanged SQL: nothing re-runs.
    let report = Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
    assert!(
        report.is_empty(),
        "an unchanged repeatable migration must not re-run"
    );
    assert_eq!(role_names(&config), ["admin"]);

    let before = recorded_repeatable_rows(&config);
    assert_eq!(2, before.len());
    assert!(before[1].3, "the repeatable row records its kind");
    assert!(!before[0].3, "the versioned row does not");

    // Changing the repeatable migration's SQL is the signal to re-run it. The
    // config carries the same live in-memory connection, so the seeded rows
    // survive into the new config.
    let mut changed = config.clone();
    changed
        .use_migrations(&[
            EmbeddedMigration::with_tag("create-roles")
                .up("create table roles (name text);")
                .down("drop table roles;")
                .boxed(),
            EmbeddedMigration::with_tag("seed-roles")
                .repeatable()
                .up("insert into roles (name) values ('editor');")
                .boxed(),
        ])
        .unwrap();
    let report = Migrator::with_config(&changed)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
    assert_eq!(report.tags(), ["seed-roles"]);
    assert_eq!(report.repeatable_tags(), ["seed-roles"]);
    assert_eq!(role_names(&changed), ["admin", "editor"]);

    // REPEAT-6: still one row for the tag, updated in place with the new
    // checksum and keeping its original id.
    let after = recorded_repeatable_rows(&changed);
    assert_eq!(2, after.len(), "a re-run must not insert a second row");
    assert_eq!(before[1].0, after[1].0, "the row keeps its id");
    assert_eq!("seed-roles", after[1].1);
    assert_ne!(before[1].2, after[1].2, "the checksum is updated");
    assert!(after[1].3, "the row is still marked repeatable");
}

// REPEAT-2
#[test]
fn a_changed_repeatable_migration_is_not_checksum_drift() {
    // The same recorded-vs-current checksum mismatch that aborts a run for a
    // versioned migration drives the re-run for a repeatable one, with the
    // drift check left at its strict default.
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let config = repeatable_config(&settings, "insert into roles (name) values ('admin');");
    config.setup().unwrap();
    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();

    let mut changed = config.clone();
    changed
        .use_migrations(&[
            EmbeddedMigration::with_tag("create-roles")
                .up("create table roles (name text);")
                .down("drop table roles;")
                .boxed(),
            EmbeddedMigration::with_tag("seed-roles")
                .repeatable()
                .up("insert into roles (name) values ('editor');")
                .boxed(),
        ])
        .unwrap();
    let report = Migrator::with_config(&changed)
        .all(true)
        .show_output(false)
        .apply()
        .expect("a repeatable migration's changed checksum is not drift");
    assert_eq!(report.repeatable_tags(), ["seed-roles"]);
}

// REPEAT-4
#[test]
fn down_runs_never_revert_repeatable_migrations() {
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let config = repeatable_config(&settings, "insert into roles (name) values ('admin');");
    config.setup().unwrap();
    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
    let config = config.reload().unwrap();
    assert_eq!(applied_tags(&config), ["create-roles", "seed-roles"]);

    // Reverting everything targets only the versioned migration; the
    // repeatable row is left in place.
    let report = Migrator::with_config(&config)
        .direction(Direction::Down)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
    assert_eq!(report.tags(), ["create-roles"]);
    assert!(report.repeatable_tags().is_empty());
    let config = config.reload().unwrap();
    assert_eq!(applied_tags(&config), ["seed-roles"]);
    assert!(!table_exists(&config, "roles"), "the down migration ran");

    // REPEAT-11: re-applying restores the versioned migration, and the
    // unchanged repeatable one stays skipped.
    let report = Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
    assert_eq!(report.tags(), ["create-roles"]);
    assert!(
        report.repeatable_tags().is_empty(),
        "an unchanged repeatable migration stays skipped after a down/up cycle"
    );
}

// REPEAT-1
#[test]
fn an_all_run_terminates_with_repeatable_migrations_present() {
    // A repeatable migration runs at most once per run, so an `all` run cannot
    // loop on one. This test would hang rather than fail if that broke.
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let config = repeatable_config(&settings, "insert into roles (name) values ('admin');");
    config.setup().unwrap();
    let report = Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
    assert_eq!(report.repeatable_tags(), ["seed-roles"]);
    assert_eq!(role_names(&config), ["admin"], "the seed ran exactly once");
}

// REPEAT-8
#[test]
fn statuses_report_repeatable_and_stale_state() {
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let config = repeatable_config(&settings, "insert into roles (name) values ('admin');");
    config.setup().unwrap();

    // Before any run both migrations are stale (they will run next).
    let config = config.reload().unwrap();
    let statuses = migrant_lib::migration_statuses(&config).unwrap();
    assert!(statuses.iter().all(|s| s.stale() && !s.applied()));
    assert!(!statuses[0].repeatable());
    assert!(statuses[1].repeatable());
    assert_eq!(
        migrant_lib::pending_migrations(&config).unwrap(),
        ["create-roles", "seed-roles"]
    );

    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
    let config = config.reload().unwrap();
    let statuses = migrant_lib::migration_statuses(&config).unwrap();
    assert!(
        statuses.iter().all(|s| s.applied() && !s.stale()),
        "everything is applied and up to date after a run"
    );
    assert!(migrant_lib::pending_migrations(&config).unwrap().is_empty());

    // Changing the repeatable migration's SQL makes it stale again while it
    // stays applied.
    let mut changed = config.clone();
    changed
        .use_migrations(&[
            EmbeddedMigration::with_tag("create-roles")
                .up("create table roles (name text);")
                .down("drop table roles;")
                .boxed(),
            EmbeddedMigration::with_tag("seed-roles")
                .repeatable()
                .up("insert into roles (name) values ('editor');")
                .boxed(),
        ])
        .unwrap();
    let changed = changed.reload().unwrap();
    let statuses = migrant_lib::migration_statuses(&changed).unwrap();
    assert!(!statuses[0].stale(), "the versioned migration is unchanged");
    assert!(
        statuses[1].applied() && statuses[1].stale() && statuses[1].repeatable(),
        "the repeatable migration is recorded but due to re-run: {statuses:?}"
    );
    assert_eq!(
        migrant_lib::pending_migrations(&changed).unwrap(),
        ["seed-roles"],
        "a stale repeatable migration is pending"
    );
}

// REPEAT-5
#[test]
fn repeatable_migrations_run_after_all_pending_versioned_ones() {
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let mut config = Config::with_settings(settings);
    config
        .use_migrations(&[
            // Declared first, but must still run last.
            EmbeddedMigration::with_tag("seed-roles")
                .repeatable()
                .up("insert into roles (name) values ('admin');")
                .boxed(),
            EmbeddedMigration::with_tag("create-roles")
                .up("create table roles (name text);")
                .down("drop table roles;")
                .boxed(),
            EmbeddedMigration::with_tag("add-index")
                .up("create index roles_name on roles (name);")
                .down("drop index roles_name;")
                .boxed(),
        ])
        .unwrap();
    config.setup().unwrap();

    let report = Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
    assert_eq!(
        report.tags(),
        ["create-roles", "add-index", "seed-roles"],
        "the repeatable migration runs after the versioned sequence"
    );
}

// REPEAT-9
#[test]
fn a_failed_repeatable_rerun_rolls_back_with_its_bookkeeping() {
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let config = repeatable_config(&settings, "insert into roles (name) values ('admin');");
    config.setup().unwrap();
    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
    let before = recorded_repeatable_rows(&config);

    // Re-run with SQL that inserts a row and then fails. Both the insert and
    // the in-place bookkeeping update must roll back together.
    let mut broken = config.clone();
    broken
        .use_migrations(&[
            EmbeddedMigration::with_tag("create-roles")
                .up("create table roles (name text);")
                .down("drop table roles;")
                .boxed(),
            EmbeddedMigration::with_tag("seed-roles")
                .repeatable()
                .up("insert into roles (name) values ('editor'); insert into nope values (1);")
                .boxed(),
        ])
        .unwrap();
    let res = Migrator::with_config(&broken)
        .all(true)
        .show_output(false)
        .apply();
    assert!(res.is_err(), "the failing re-run must abort the run");
    assert_eq!(
        role_names(&broken),
        ["admin"],
        "the partial insert rolled back"
    );
    assert_eq!(
        before,
        recorded_repeatable_rows(&broken),
        "the recorded checksum is unchanged, so the migration is retried next run"
    );
}

// REPEAT-9
#[test]
fn fake_records_a_repeatable_rerun_without_running_its_sql() {
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let config = repeatable_config(&settings, "insert into roles (name) values ('admin');");
    config.setup().unwrap();
    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
    assert_eq!(role_names(&config), ["admin"]);

    // A faked re-run updates the recorded checksum without executing the SQL,
    // so the migration is no longer stale but nothing was seeded.
    let mut changed = config.clone();
    changed
        .use_migrations(&[
            EmbeddedMigration::with_tag("create-roles")
                .up("create table roles (name text);")
                .down("drop table roles;")
                .boxed(),
            EmbeddedMigration::with_tag("seed-roles")
                .repeatable()
                .up("insert into roles (name) values ('editor');")
                .boxed(),
        ])
        .unwrap();
    let report = Migrator::with_config(&changed)
        .all(true)
        .fake(true)
        .show_output(false)
        .apply()
        .unwrap();
    assert_eq!(report.repeatable_tags(), ["seed-roles"]);
    assert_eq!(
        role_names(&changed),
        ["admin"],
        "a faked re-run must not run the SQL"
    );

    // The recorded checksum was updated, so it is no longer stale.
    let changed = changed.reload().unwrap();
    let statuses = migrant_lib::migration_statuses(&changed).unwrap();
    assert!(!statuses[1].stale(), "a faked re-run records the checksum");
}

// REPEAT-9
#[test]
fn force_skip_failures_retries_a_repeatable_rerun_next_run() {
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let config = repeatable_config(&settings, "insert into roles (name) values ('admin');");
    config.setup().unwrap();
    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();

    // A failing re-run under skip-failures leaves the recorded checksum alone,
    // and the run still terminates rather than retrying the same migration.
    let mut broken = config.clone();
    broken
        .use_migrations(&[
            EmbeddedMigration::with_tag("create-roles")
                .up("create table roles (name text);")
                .down("drop table roles;")
                .boxed(),
            EmbeddedMigration::with_tag("seed-roles")
                .repeatable()
                .up("insert into nope values (1);")
                .boxed(),
        ])
        .unwrap();
    let report = Migrator::with_config(&broken)
        .all(true)
        .force(ForceMode::SkipFailures)
        .show_output(false)
        .apply()
        .unwrap();
    assert!(
        report.is_empty(),
        "a skipped re-run records nothing: {report:?}"
    );

    // Still stale, so the next run retries it.
    let broken = broken.reload().unwrap();
    let statuses = migrant_lib::migration_statuses(&broken).unwrap();
    assert!(
        statuses[1].stale(),
        "an unrecorded failed re-run is retried on the next run"
    );
}

// REPEAT-9
#[test]
fn force_accept_failures_records_a_failed_repeatable_rerun_in_place() {
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let config = repeatable_config(&settings, "insert into roles (name) values ('admin');");
    config.setup().unwrap();
    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
    let before = recorded_repeatable_rows(&config);

    // Under accept-failures the re-run fails, its SQL is rolled back, but the
    // bookkeeping is still updated: the migration is marked up to date and is
    // NOT retried on the next run.
    let mut broken = config.clone();
    broken
        .use_migrations(&[
            EmbeddedMigration::with_tag("create-roles")
                .up("create table roles (name text);")
                .down("drop table roles;")
                .boxed(),
            EmbeddedMigration::with_tag("seed-roles")
                .repeatable()
                .up("insert into roles (name) values ('editor'); insert into nope values (1);")
                .boxed(),
        ])
        .unwrap();
    let report = Migrator::with_config(&broken)
        .all(true)
        .force(ForceMode::AcceptFailures)
        .show_output(false)
        .apply()
        .unwrap();
    assert_eq!(report.repeatable_tags(), ["seed-roles"]);
    assert_eq!(
        role_names(&broken),
        ["admin"],
        "the failed re-run's SQL rolled back"
    );

    // The row was updated in place: still one row, same id, new checksum.
    let after = recorded_repeatable_rows(&broken);
    assert_eq!(before.len(), after.len(), "no extra row was inserted");
    assert_eq!(before[1].0, after[1].0, "the row keeps its id");
    assert_ne!(before[1].2, after[1].2, "the checksum was rewritten");

    // Marked up to date despite having failed, which is the documented
    // accept-failures tradeoff.
    let broken = broken.reload().unwrap();
    let statuses = migrant_lib::migration_statuses(&broken).unwrap();
    assert!(
        !statuses[1].stale(),
        "accept-failures records the re-run, so it is not retried"
    );
}

// REPEAT-12
#[test]
fn rerun_repeatable_reruns_an_unchanged_migration() {
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let config = repeatable_config(&settings, "insert into roles (name) values ('admin');");
    config.setup().unwrap();
    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
    assert_eq!(role_names(&config), ["admin"]);
    let before = recorded_repeatable_rows(&config);

    // Without the override an unchanged repeatable migration is skipped.
    let report = Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
    assert!(report.is_empty());

    // With it, the SQL runs again even though nothing changed.
    let report = Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .rerun_repeatable(true)
        .apply()
        .unwrap();
    assert_eq!(report.tags(), ["seed-roles"]);
    assert_eq!(report.repeatable_tags(), ["seed-roles"]);
    assert_eq!(
        role_names(&config),
        ["admin", "admin"],
        "the seed ran a second time"
    );

    // Bookkeeping is still one row updated in place, with the same (unchanged)
    // checksum and the same id.
    let after = recorded_repeatable_rows(&config);
    assert_eq!(before.len(), after.len(), "no extra row was inserted");
    assert_eq!(before[1].0, after[1].0, "the row keeps its id");
    assert_eq!(before[1].2, after[1].2, "the checksum is unchanged");
    assert!(after[1].3);

    // The run terminates: the override does not defeat the once-per-run guard.
    let report = Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .rerun_repeatable(true)
        .apply()
        .unwrap();
    assert_eq!(
        report.repeatable_tags(),
        ["seed-roles"],
        "each override run re-runs it exactly once"
    );
    assert_eq!(role_names(&config), ["admin", "admin", "admin"]);
}

// REPEAT-12
#[test]
fn rerun_repeatable_does_not_re_apply_versioned_migrations() {
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let config = repeatable_config(&settings, "insert into roles (name) values ('admin');");
    config.setup().unwrap();
    Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();

    // `create-roles` would fail if re-applied (the table exists), so a passing
    // run proves the override left it alone.
    let report = Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .rerun_repeatable(true)
        .apply()
        .unwrap();
    assert_eq!(
        report.tags(),
        ["seed-roles"],
        "only the repeatable migration re-ran"
    );
}

// REPEAT-7
#[test]
fn registering_an_invalid_repeatable_migration_errors() {
    // A repeatable migration with a down direction.
    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let mut config = Config::with_settings(settings.clone());
    let res = config.use_migrations(&[EmbeddedMigration::with_tag("seed-roles")
        .repeatable()
        .up("select 1;")
        .down("select -1;")
        .boxed()]);
    assert!(
        res.is_err(),
        "a repeatable migration must not define a down direction"
    );

    // A repeatable migration with no up-SQL, so no checksum.
    let mut config = Config::with_settings(settings);
    let res = config.use_migrations(&[EmbeddedMigration::with_tag("seed-roles")
        .repeatable()
        .boxed()]);
    assert!(
        res.is_err(),
        "a repeatable migration must have a checksum to compare"
    );
}

// REPEAT-3
#[test]
fn file_migrations_declare_repeatable_through_the_up_file_directive() {
    // The directive is the CLI's route to a repeatable migration: the file set
    // is discovered from disk, with no builder call available.
    let dir = tempfile::tempdir().unwrap();
    let up = dir.path().join("up.sql");
    std::fs::write(
        &up,
        b"-- migrant:repeatable\ninsert into roles (name) values ('admin');",
    )
    .unwrap();

    let settings = Settings::configure_sqlite().memory().build().unwrap();
    let mut config = Config::with_settings(settings);
    config
        .use_migrations(&[
            EmbeddedMigration::with_tag("create-roles")
                .up("create table roles (name text);")
                .down("drop table roles;")
                .boxed(),
            FileMigration::with_tag("seed-roles").up(&up).boxed(),
        ])
        .unwrap();
    config.setup().unwrap();

    let report = Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
    assert_eq!(
        report.repeatable_tags(),
        ["seed-roles"],
        "the up-file directive declares the migration repeatable"
    );

    // Editing the file changes its checksum, which re-runs it.
    std::fs::write(
        &up,
        b"-- migrant:repeatable\ninsert into roles (name) values ('editor');",
    )
    .unwrap();
    let report = Migrator::with_config(&config)
        .all(true)
        .show_output(false)
        .apply()
        .unwrap();
    assert_eq!(report.repeatable_tags(), ["seed-roles"]);
    assert_eq!(role_names(&config), ["admin", "editor"]);
}
