//! CLI integration tests
//!
//! These run the compiled `migrant` binary against the repo's own
//! `Migrant.toml` (sqlite) and `migrations/` directory, so they require
//! the `sqlite` feature and mutate `db/migrant.db`:
//!
//! ```text
//! cargo test --features sqlite,integration_tests
//! ```
#![cfg(all(feature = "integration_tests", feature = "sqlite"))]

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;

fn migrant() -> Command {
    Command::cargo_bin("migrant").expect("binary built")
}

#[test]
fn kitchen_sink() {
    // make sure we're setup and back to no applied migrations. `--step` with
    // a count comfortably larger than the number of migrations reverts
    // everything and stops early once nothing remains.
    migrant().arg("setup").assert().success();
    let _ = migrant().args(["apply", "-d", "--step", "100"]).assert();

    // A down run with nothing left to un-apply is not an error; it succeeds and
    // reports the (all-unapplied) status.
    migrant()
        .args(["apply", "-d", "--step", "100"])
        .assert()
        .success()
        .stdout(contains("[ ] 20170812145327_initial"))
        .stdout(contains("[ ] 20171126194042_second"));

    migrant()
        .arg("list")
        .assert()
        .success()
        .stdout(contains("Current Migration Status:"))
        .stdout(contains("[ ] 20170812145327_initial"))
        .stdout(contains("[ ] 20171126194042_second"));

    // `apply` with no flags applies all pending migrations in one invocation.
    migrant()
        .arg("apply")
        .assert()
        .success()
        .stdout(contains("Applying[Up]:"))
        .stdout(contains("Current Migration Status:"))
        .stdout(contains("[✓] 20170812145327_initial"))
        .stdout(contains("[✓] 20171126194042_second"));

    migrant()
        .arg("list")
        .assert()
        .success()
        .stdout(contains("Current Migration Status:"))
        .stdout(contains("[✓] 20170812145327_initial"))
        .stdout(contains("[✓] 20171126194042_second"));

    migrant()
        .arg("redo")
        .assert()
        .success()
        .stdout(contains("Applying[Down]:"))
        .stdout(contains("[ ] 20171126194042_second"))
        .stdout(contains("Applying[Up]:"))
        .stdout(contains("[✓] 20171126194042_second"));

    migrant()
        .args(["redo", "--all"])
        .assert()
        .success()
        .stdout(contains("Applying[Down]:"))
        .stdout(contains("[ ] 20170812145327_initial"))
        .stdout(contains("[ ] 20171126194042_second"))
        .stdout(contains("Applying[Up]:"))
        .stdout(contains("[✓] 20170812145327_initial"))
        .stdout(contains("[✓] 20171126194042_second"));

    migrant()
        .arg("connect-string")
        .assert()
        .success()
        .stdout(contains("db/migrant.db"));

    migrant()
        .arg("which-config")
        .assert()
        .success()
        .stdout(contains("Migrant.toml"));

    let _ = migrant().args(["apply", "-d", "--step", "100"]).assert();
}

// CLIMIG-6: `status` reports every managed migration in text and json.
#[test]
fn status_reports_text_and_json() {
    let dir = sqlite_project();
    migrant()
        .current_dir(dir.path())
        .arg("setup")
        .assert()
        .success();
    new_migration(
        dir.path(),
        "first",
        "create table status_a (x integer);",
        "drop table status_a;",
    );
    new_migration(
        dir.path(),
        "second",
        "create table status_b (x integer);",
        "drop table status_b;",
    );

    // `apply` with no flags now applies every pending migration, so use
    // `--step 1` to leave one applied and one pending. `new_migration`
    // guarantees the two migrations landed in distinct seconds, but the
    // status/list output is still not committed to a specific tag ordering
    // here, so assert on the mixed state and counts rather than which
    // specific tag ends up applied.
    migrant()
        .current_dir(dir.path())
        .args(["apply", "--step", "1"])
        .assert()
        .success();

    // default (text) format: summary line plus one applied and one pending row
    migrant()
        .current_dir(dir.path())
        .arg("status")
        .assert()
        .success()
        .stdout(contains("Migration status: 1 applied, 1 pending (2 total)"))
        .stdout(predicates::str::is_match(r"\[✓\] \d{14}_").expect("valid regex"))
        .stdout(predicates::str::is_match(r"\[ \] \d{14}_").expect("valid regex"));

    // json format is valid and carries the same counts
    let out = migrant()
        .current_dir(dir.path())
        .args(["status", "--format", "json"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).expect("utf8 stdout");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");
    assert_eq!(value["total"], 2);
    assert_eq!(value["applied"], 1);
    assert_eq!(value["pending"], 1);
    let migrations = value["migrations"].as_array().expect("array");
    assert_eq!(migrations.len(), 2);
    assert_eq!(
        migrations.iter().filter(|m| m["applied"] == true).count(),
        1
    );
}

// TUI-1: with stdout piped (not a terminal) the tui refuses to start,
// before touching the database
#[test]
fn tui_requires_an_interactive_terminal() {
    migrant()
        .arg("tui")
        .assert()
        .failure()
        .stderr(contains("requires an interactive terminal"));
}

/// A tempdir with a sqlite `Migrant.toml`, isolated from the repo's own
/// config (and from the other tests, so these run in parallel safely).
fn sqlite_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create tempdir");
    std::fs::write(
        dir.path().join("Migrant.toml"),
        "database_type = \"sqlite\"\n\
         database_path = \"db.db\"\n\
         migration_location = \"migrations\"\n",
    )
    .expect("write Migrant.toml");
    dir
}

/// Create a migration via `migrant new` and overwrite its up/down files.
///
/// No inter-migration timing dance is needed: `migrant new`'s generated tag has
/// second-resolution timestamps, but the library now discovers/orders
/// migrations by a deterministic total order (timestamp, then tag), so
/// migrations created back-to-back in the same second still have a stable
/// definition order. (See `same_second_migrations_apply_deterministically` for
/// the regression that guards this.) Callers that assert on relative apply order
/// pick tags whose intended order they control.
fn new_migration(dir: &std::path::Path, tag: &str, up: &str, down: &str) {
    migrant()
        .current_dir(dir)
        .args(["new", tag])
        .assert()
        .success();
    let migrations = dir.join("migrations");
    let mig_dir = std::fs::read_dir(&migrations)
        .expect("read migrations dir")
        .map(|e| e.expect("dir entry").path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(&format!("_{}", tag)))
        })
        .unwrap_or_else(|| panic!("migration dir for `{}` not found", tag));
    std::fs::write(mig_dir.join("up.sql"), up).expect("write up.sql");
    std::fs::write(mig_dir.join("down.sql"), down).expect("write down.sql");
}

/// Create a migration directory directly on disk with a caller-chosen 14-digit
/// timestamp `stamp` and `tag`, bypassing `migrant new`. This is the only way
/// to force two migrations to share the exact same timestamp second (which
/// `migrant new` + [`wait_for_distinct_migration_second`] deliberately avoids).
fn raw_migration(dir: &std::path::Path, stamp: &str, tag: &str, up: &str, down: &str) {
    let mig_dir = dir.join("migrations").join(format!("{}_{}", stamp, tag));
    std::fs::create_dir_all(&mig_dir).expect("create migration dir");
    std::fs::write(mig_dir.join("up.sql"), up).expect("write up.sql");
    std::fs::write(mig_dir.join("down.sql"), down).expect("write down.sql");
}

/// Delete the on-disk migration directory whose name ends in `_<tag>`, so its
/// applied tag becomes "unknown" (recorded as applied but absent from the
/// available set).
fn remove_migration(dir: &std::path::Path, tag: &str) {
    let migrations = dir.join("migrations");
    let mig_dir = std::fs::read_dir(&migrations)
        .expect("read migrations dir")
        .map(|e| e.expect("dir entry").path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(&format!("_{}", tag)))
        })
        .unwrap_or_else(|| panic!("migration dir for `{}` not found", tag));
    std::fs::remove_dir_all(&mig_dir).expect("remove migration dir");
}

/// Rewrite the up.sql of the on-disk migration whose name ends in `_<tag>`,
/// changing its checksum as if the file were edited after it was applied.
fn edit_migration_up(dir: &std::path::Path, tag: &str, up: &str) {
    let migrations = dir.join("migrations");
    let mig_dir = std::fs::read_dir(&migrations)
        .expect("read migrations dir")
        .map(|e| e.expect("dir entry").path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(&format!("_{}", tag)))
        })
        .unwrap_or_else(|| panic!("migration dir for `{}` not found", tag));
    std::fs::write(mig_dir.join("up.sql"), up).expect("write up.sql");
}

// CLIMIG: `apply --step N --down` reverts at most N applied migrations
// (newest-first) and stops early once nothing remains, mirroring the up
// direction. Only the up direction was exercised before.
#[test]
fn apply_step_down_reverts_limited_and_stops_early() {
    let dir = sqlite_project();
    migrant()
        .current_dir(dir.path())
        .arg("setup")
        .assert()
        .success();
    new_migration(
        dir.path(),
        "one",
        "create table step_down_a (x integer);",
        "drop table step_down_a;",
    );
    new_migration(
        dir.path(),
        "two",
        "create table step_down_b (x integer);",
        "drop table step_down_b;",
    );
    new_migration(
        dir.path(),
        "three",
        "create table step_down_c (x integer);",
        "drop table step_down_c;",
    );

    // Apply everything up first.
    migrant()
        .current_dir(dir.path())
        .arg("apply")
        .assert()
        .success();

    // `--step 2 --down` reverts exactly the two most-recently applied
    // (`three`, then `two`), leaving only `one` applied.
    migrant()
        .current_dir(dir.path())
        .args(["apply", "--step", "2", "--down"])
        .assert()
        .success();
    migrant()
        .current_dir(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicates::str::is_match(r"\[✓\] \d{14}_one").expect("valid regex"))
        .stdout(predicates::str::is_match(r"\[ \] \d{14}_two").expect("valid regex"))
        .stdout(predicates::str::is_match(r"\[ \] \d{14}_three").expect("valid regex"));

    // `--step 100 --down` with only one applied reverts it and stops early
    // instead of erroring once nothing remains.
    migrant()
        .current_dir(dir.path())
        .args(["apply", "--step", "100", "--down"])
        .assert()
        .success();
    migrant()
        .current_dir(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicates::str::is_match(r"\[ \] \d{14}_one").expect("valid regex"))
        .stdout(predicates::str::is_match(r"\[ \] \d{14}_two").expect("valid regex"))
        .stdout(predicates::str::is_match(r"\[ \] \d{14}_three").expect("valid regex"));
}

// CLIMIG-4: end-to-end unknown-applied-tag strictness. With an applied tag that
// is no longer among the available migrations, a plain `apply` aborts; the same
// run with `--allow-unknown-tags` succeeds. (Flag *acceptance* is covered
// elsewhere; this drives the actual error path and the opt-out.)
#[test]
fn apply_unknown_tag_rejected_then_allowed() {
    let dir = sqlite_project();
    migrant()
        .current_dir(dir.path())
        .arg("setup")
        .assert()
        .success();
    new_migration(
        dir.path(),
        "solo",
        "create table unknown_solo (x integer);",
        "drop table unknown_solo;",
    );

    // Apply it, then delete its files so the applied tag becomes unknown.
    migrant()
        .current_dir(dir.path())
        .arg("apply")
        .assert()
        .success();
    remove_migration(dir.path(), "solo");

    // A plain up run enforces the unknown-tag check and aborts.
    migrant()
        .current_dir(dir.path())
        .arg("apply")
        .assert()
        .failure()
        .stderr(contains("MigrationNotFound"))
        .stderr(contains("is not among the available migrations"));

    // Opting out lets the run proceed (nothing left to apply).
    migrant()
        .current_dir(dir.path())
        .args(["apply", "--allow-unknown-tags"])
        .assert()
        .success();
}

// CLIMIG-4: end-to-end out-of-order strictness. `--force=skip-failures` leaves a
// later migration applied ahead of an earlier, still-pending one; a plain
// `apply` then aborts as out-of-order, while `--allow-out-of-order` proceeds.
#[test]
fn apply_out_of_order_rejected_then_allowed() {
    let dir = sqlite_project();
    migrant()
        .current_dir(dir.path())
        .arg("setup")
        .assert()
        .success();
    // `a-bad` is defined first (earlier second) but its SQL fails; `b-good`
    // is defined second and succeeds.
    new_migration(
        dir.path(),
        "a-bad",
        "insert into does_not_exist values (1);",
        "select 1;",
    );
    new_migration(
        dir.path(),
        "b-good",
        "create table ooo_good (x integer);",
        "drop table ooo_good;",
    );

    // skip-failures leaves `b-good` applied ahead of the still-pending `a-bad`.
    migrant()
        .current_dir(dir.path())
        .args(["apply", "--force=skip-failures"])
        .assert()
        .success();
    migrant()
        .current_dir(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicates::str::is_match(r"\[ \] \d{14}_a-bad").expect("valid regex"))
        .stdout(predicates::str::is_match(r"\[✓\] \d{14}_b-good").expect("valid regex"));

    // A plain up run detects the out-of-order applied set and aborts.
    migrant()
        .current_dir(dir.path())
        .arg("apply")
        .assert()
        .failure()
        .stderr(contains("MigrationOrdering"))
        .stderr(contains("out of order"));

    // Opting out lets the run proceed past the intervening migration.
    // (`a-bad` still fails, so `--force` records it and the run completes.)
    migrant()
        .current_dir(dir.path())
        .args(["apply", "--allow-out-of-order", "--force"])
        .assert()
        .success();
    migrant()
        .current_dir(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicates::str::is_match(r"\[✓\] \d{14}_a-bad").expect("valid regex"));
}

// DRIFT-1/DRIFT-6: an already-applied migration whose up.sql is edited aborts a
// following `apply` with a checksum mismatch, and `--allow-checksum-mismatch`
// proceeds past the drift.
#[test]
fn apply_checksum_drift_rejected_then_allowed() {
    let dir = sqlite_project();
    migrant()
        .current_dir(dir.path())
        .arg("setup")
        .assert()
        .success();
    new_migration(
        dir.path(),
        "seed",
        "create table seed (x integer);",
        "drop table seed;",
    );
    migrant()
        .current_dir(dir.path())
        .arg("apply")
        .assert()
        .success();

    // Edit the applied migration's up.sql: its checksum no longer matches.
    edit_migration_up(
        dir.path(),
        "seed",
        "create table seed (x integer, y integer);",
    );

    // A plain up run detects the drift and aborts before applying anything.
    migrant()
        .current_dir(dir.path())
        .arg("apply")
        .assert()
        .failure()
        .stderr(contains("ChecksumMismatch"))
        .stderr(contains("has changed since it was applied"));

    // Opting out lets the run proceed (the migration is already applied, so
    // there is nothing left to do).
    migrant()
        .current_dir(dir.path())
        .args(["apply", "--allow-checksum-mismatch"])
        .assert()
        .success();
}

// CLIMIG-3: `new` reports the created up/down file paths on stdout.
#[test]
fn new_reports_created_paths() {
    let dir = sqlite_project();
    migrant()
        .current_dir(dir.path())
        .arg("setup")
        .assert()
        .success();

    migrant()
        .current_dir(dir.path())
        .args(["new", "reported"])
        .assert()
        .success()
        .stdout(
            predicates::str::is_match(r"Created: .*_reported[\\/]+up\.sql").expect("valid regex"),
        )
        .stdout(
            predicates::str::is_match(r"Created: .*_reported[\\/]+down\.sql").expect("valid regex"),
        );
}

/// Create a repeatable migration via `migrant new --repeatable` and overwrite
/// its up file, keeping the `-- migrant:repeatable` directive.
fn new_repeatable_migration(dir: &std::path::Path, tag: &str, up: &str) {
    migrant()
        .current_dir(dir)
        .args(["new", "--repeatable", tag])
        .assert()
        .success();
    edit_migration_up(dir, tag, &format!("-- migrant:repeatable\n{}", up));
}

// CLIMIG-1/REPEAT-10: `new --repeatable` creates only a seeded up.sql.
#[test]
fn new_repeatable_creates_only_a_seeded_up_file() {
    let dir = sqlite_project();
    migrant()
        .current_dir(dir.path())
        .arg("setup")
        .assert()
        .success();

    migrant()
        .current_dir(dir.path())
        .args(["new", "--repeatable", "seed-roles"])
        .assert()
        .success()
        .stdout(
            predicates::str::is_match(r"Created: .*_seed-roles[\\/]+up\.sql").expect("valid regex"),
        )
        .stdout(contains("down.sql").not());

    let mig_dir = std::fs::read_dir(dir.path().join("migrations"))
        .expect("read migrations dir")
        .map(|e| e.expect("dir entry").path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with("_seed-roles"))
        })
        .expect("migration dir created");
    assert!(!mig_dir.join("down.sql").exists(), "no down.sql is created");
    let up = std::fs::read_to_string(mig_dir.join("up.sql")).expect("read up.sql");
    assert_eq!("-- migrant:repeatable\n", up);
}

// REPEAT-1/REPEAT-8/CLIMIG-6: a repeatable migration re-runs when its up file
// changes, and `status` reports it as repeatable and stale beforehand.
#[test]
fn repeatable_migration_reruns_through_the_cli() {
    let dir = sqlite_project();
    migrant()
        .current_dir(dir.path())
        .arg("setup")
        .assert()
        .success();
    new_migration(
        dir.path(),
        "create-roles",
        "create table roles (name text);",
        "drop table roles;",
    );
    new_repeatable_migration(
        dir.path(),
        "seed-roles",
        "insert into roles (name) values ('admin');",
    );

    // Before the first run the repeatable migration is pending and stale.
    migrant()
        .current_dir(dir.path())
        .args(["status", "--format", "json"])
        .assert()
        .success()
        .stdout(contains("\"repeatable\": true"))
        .stdout(contains("\"stale\": true"));

    migrant()
        .current_dir(dir.path())
        .arg("apply")
        .assert()
        .success();

    // Applied and up to date: annotated but not marked for a re-run.
    migrant()
        .current_dir(dir.path())
        .arg("status")
        .assert()
        .success()
        .stdout(contains("(repeatable)"))
        .stdout(contains("will re-run").not())
        .stdout(contains("0 pending"));

    // Re-running with nothing changed does not re-apply it.
    migrant()
        .current_dir(dir.path())
        .arg("apply")
        .assert()
        .success()
        .stdout(contains("Re-applying").not());

    // Editing the up file changes its checksum, which is the re-run signal
    // rather than the drift error a versioned migration would raise.
    edit_migration_up(
        dir.path(),
        "seed-roles",
        "-- migrant:repeatable\ninsert into roles (name) values ('editor');",
    );
    migrant()
        .current_dir(dir.path())
        .arg("status")
        .assert()
        .success()
        .stdout(contains("(repeatable, will re-run)"))
        .stdout(contains("1 stale"));

    migrant()
        .current_dir(dir.path())
        .arg("apply")
        .assert()
        .success()
        .stdout(contains("Re-applying[Up]"));

    // Both seeds ran, and the tag still has exactly one bookkeeping row.
    let db = rusqlite::Connection::open(dir.path().join("db.db")).expect("open db");
    let names: Vec<String> = db
        .prepare("select name from roles order by name")
        .expect("prepare")
        .query_map([], |r| r.get(0))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows");
    assert_eq!(names, ["admin", "editor"]);
    let rows: i64 = db
        .query_row(
            "select count(*) from __migrant_migrations where tag like '%seed-roles'",
            [],
            |r| r.get(0),
        )
        .expect("count rows");
    assert_eq!(1, rows, "a re-run updates the row in place");
    let repeatable: bool = db
        .query_row(
            "select is_repeatable from __migrant_migrations where tag like '%seed-roles'",
            [],
            |r| r.get(0),
        )
        .expect("read is_repeatable");
    assert!(repeatable, "the row records the migration's kind");
}

// REPEAT-12: `apply --rerun-repeatable` re-runs an unchanged repeatable
// migration; without it nothing runs.
#[test]
fn apply_rerun_repeatable_runs_unchanged_migrations() {
    let dir = sqlite_project();
    migrant()
        .current_dir(dir.path())
        .arg("setup")
        .assert()
        .success();
    new_migration(
        dir.path(),
        "create-roles",
        "create table roles (name text);",
        "drop table roles;",
    );
    new_repeatable_migration(
        dir.path(),
        "seed-roles",
        "insert into roles (name) values ('admin');",
    );
    migrant()
        .current_dir(dir.path())
        .arg("apply")
        .assert()
        .success();

    // Nothing changed, so a plain re-apply does nothing.
    migrant()
        .current_dir(dir.path())
        .arg("apply")
        .assert()
        .success()
        .stdout(contains("Re-applying").not());

    // The flag runs it anyway.
    migrant()
        .current_dir(dir.path())
        .args(["apply", "--rerun-repeatable"])
        .assert()
        .success()
        .stdout(contains("Re-applying[Up]"));

    let db = rusqlite::Connection::open(dir.path().join("db.db")).expect("open db");
    let count: i64 = db
        .query_row("select count(*) from roles", [], |r| r.get(0))
        .expect("count roles");
    assert_eq!(2, count, "the seed ran a second time");
    // Still one bookkeeping row for the tag.
    let rows: i64 = db
        .query_row(
            "select count(*) from __migrant_migrations where tag like '%seed-roles'",
            [],
            |r| r.get(0),
        )
        .expect("count rows");
    assert_eq!(1, rows);
}

// REPEAT-13: `redo` warns that it does not revert repeatable migrations.
#[test]
fn redo_warns_that_it_skips_repeatable_migrations() {
    let dir = sqlite_project();
    migrant()
        .current_dir(dir.path())
        .arg("setup")
        .assert()
        .success();
    new_migration(
        dir.path(),
        "create-roles",
        "create table roles (name text);",
        "drop table roles;",
    );

    // With no repeatable migrations there is nothing to warn about.
    migrant()
        .current_dir(dir.path())
        .arg("apply")
        .assert()
        .success();
    migrant()
        .current_dir(dir.path())
        .arg("redo")
        .assert()
        .success()
        .stderr(contains("does not revert repeatable").not());

    // Once one is applied, `redo` says so, naming it, and points at the flag.
    new_repeatable_migration(
        dir.path(),
        "seed-roles",
        "insert into roles (name) values ('admin');",
    );
    migrant()
        .current_dir(dir.path())
        .arg("apply")
        .assert()
        .success();
    migrant()
        .current_dir(dir.path())
        .arg("redo")
        .assert()
        .success()
        .stderr(contains("does not revert repeatable migrations"))
        .stderr(contains("seed-roles"))
        .stderr(contains("--rerun-repeatable"));

    // `--rerun-repeatable` makes the note moot, so it is suppressed.
    migrant()
        .current_dir(dir.path())
        .args(["redo", "--rerun-repeatable"])
        .assert()
        .success()
        .stderr(contains("does not revert repeatable").not());
}

// REPEAT-4: `apply --down` never reverts a repeatable migration.
#[test]
fn apply_down_leaves_repeatable_migrations_recorded() {
    let dir = sqlite_project();
    migrant()
        .current_dir(dir.path())
        .arg("setup")
        .assert()
        .success();
    new_migration(
        dir.path(),
        "create-roles",
        "create table roles (name text);",
        "drop table roles;",
    );
    new_repeatable_migration(
        dir.path(),
        "seed-roles",
        "insert into roles (name) values ('admin');",
    );
    migrant()
        .current_dir(dir.path())
        .arg("apply")
        .assert()
        .success();

    // The repeatable migration was applied last, but the down run targets the
    // versioned one and leaves the repeatable row alone.
    migrant()
        .current_dir(dir.path())
        .args(["apply", "-d", "--step", "100"])
        .assert()
        .success();

    let db = rusqlite::Connection::open(dir.path().join("db.db")).expect("open db");
    let remaining: Vec<String> = db
        .prepare("select tag from __migrant_migrations order by id")
        .expect("prepare")
        .query_map([], |r| r.get(0))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows");
    assert_eq!(1, remaining.len(), "only the repeatable row remains");
    assert!(remaining[0].ends_with("_seed-roles"));
}

// REPEAT-4/REPEAT-7: a repeatable migration that also defines a down direction
// is rejected rather than silently ignored.
#[test]
fn repeatable_migration_with_a_down_file_is_rejected() {
    let dir = sqlite_project();
    migrant()
        .current_dir(dir.path())
        .arg("setup")
        .assert()
        .success();
    // `new` (not `new --repeatable`) writes a down.sql; adding the directive to
    // the up file then makes the pair invalid.
    new_migration(
        dir.path(),
        "seed-roles",
        "-- migrant:repeatable\ninsert into roles (name) values ('admin');",
        "delete from roles;",
    );

    migrant()
        .current_dir(dir.path())
        .arg("apply")
        .assert()
        .failure()
        .stderr(contains("seed-roles"))
        .stderr(contains("down"));
}

// REGRESSION (migrant_lib same-second ordering fix): migrations that share the
// exact same timestamp second must have a deterministic definition order.
//
// The library previously discovered migrations via a randomly-seeded `HashMap`
// and a *stable* `sort_by_key(|m| m.stamp)`, so same-second ties kept
// nondeterministic HashMap iteration order. Because each `apply_next` re-runs
// discovery, a single default `migrant apply` of same-second migrations could
// see one order on one step and a different order on the next, and the item-4
// out-of-order check would then spuriously abort a perfectly legitimate set
// with `MigrationOrdering` (~50% of runs). `migrant_lib::ops` now sorts by a
// total order (timestamp, then tag), so this test drives the default strict
// path (no `--allow-*` flags) over same-second migrations and asserts it
// succeeds deterministically -- applying all of them in a stable tag order --
// across repeated runs.
#[test]
fn same_second_migrations_apply_deterministically() {
    let dir = sqlite_project();
    // Three legitimate migrations sharing the exact same timestamp second. The
    // deterministic tiebreak is by tag, so definition order is aaa < bbb < ccc.
    raw_migration(
        dir.path(),
        "20200101000000",
        "aaa",
        "create table ss_aaa (x integer);",
        "drop table ss_aaa;",
    );
    raw_migration(
        dir.path(),
        "20200101000000",
        "bbb",
        "create table ss_bbb (x integer);",
        "drop table ss_bbb;",
    );
    raw_migration(
        dir.path(),
        "20200101000000",
        "ccc",
        "create table ss_ccc (x integer);",
        "drop table ss_ccc;",
    );
    migrant()
        .current_dir(dir.path())
        .arg("setup")
        .assert()
        .success();

    // Repeat: a default `apply` (strict checks, no allow-flags) must succeed
    // every time and apply all three. Under the old bug this aborted with
    // `MigrationOrdering` on roughly half the runs; the fix makes it stable.
    for _ in 0..10 {
        migrant()
            .current_dir(dir.path())
            .arg("apply")
            .assert()
            .success();

        let out = migrant()
            .current_dir(dir.path())
            .arg("list")
            .assert()
            .success()
            .stdout(contains("[✓] 20200101000000_aaa"))
            .stdout(contains("[✓] 20200101000000_bbb"))
            .stdout(contains("[✓] 20200101000000_ccc"));

        // Definition/list order is the deterministic tiebreak: aaa, bbb, ccc.
        let stdout = String::from_utf8(out.get_output().stdout.clone()).expect("utf8 stdout");
        let a = stdout.find("20200101000000_aaa").expect("aaa listed");
        let b = stdout.find("20200101000000_bbb").expect("bbb listed");
        let c = stdout.find("20200101000000_ccc").expect("ccc listed");
        assert!(
            a < b && b < c,
            "same-second order must be stable (aaa,bbb,ccc)"
        );

        // Reset to all-unapplied for the next iteration.
        migrant()
            .current_dir(dir.path())
            .args(["apply", "-d", "--step", "100"])
            .assert()
            .success();
    }
}

// CLIPRO-3: without a config, commands error and point at `init` instead of
// silently starting the interactive config-creation flow.
#[test]
fn no_config_errors_and_points_at_init() {
    let dir = tempfile::tempdir().expect("create tempdir");
    for cmd in ["list", "which-config", "setup", "connect-string"] {
        migrant()
            .current_dir(dir.path())
            .arg(cmd)
            .assert()
            .failure()
            .stderr(contains("No `Migrant.toml` found"))
            .stderr(contains("migrant init"));
    }
}

// CLIPRO-1: non-interactive `init` writes a config without prompting.
#[test]
fn init_non_interactive_creates_config() {
    let dir = tempfile::tempdir().expect("create tempdir");
    migrant()
        .current_dir(dir.path())
        .args(["init", "-t", "sqlite", "--no-confirm"])
        .assert()
        .success();
    let config = std::fs::read_to_string(dir.path().join("Migrant.toml"))
        .expect("Migrant.toml must be created");
    assert!(config.contains("database_type = \"sqlite\""));
}

#[test]
fn init_rejects_invalid_database_type() {
    let dir = tempfile::tempdir().expect("create tempdir");
    migrant()
        .current_dir(dir.path())
        .args(["init", "-t", "nosuchdb", "--no-confirm"])
        .assert()
        .failure()
        .stderr(contains("Invalid Database Kind"));
    assert!(
        !dir.path().join("Migrant.toml").exists(),
        "no config may be written on error"
    );
}

// CLIMIG-1: `new` validates tags before creating anything.
#[test]
fn new_rejects_invalid_tag() {
    let dir = sqlite_project();
    migrant()
        .current_dir(dir.path())
        .arg("setup")
        .assert()
        .success();
    migrant()
        .current_dir(dir.path())
        .args(["new", "Bad_Tag!"])
        .assert()
        .failure()
        .stderr(contains("Invalid tag"));
}

// CLIMIG-4: `--fake` records the migration without running its SQL.
#[test]
fn apply_fake_records_without_running() {
    let dir = sqlite_project();
    migrant()
        .current_dir(dir.path())
        .arg("setup")
        .assert()
        .success();
    new_migration(
        dir.path(),
        "first",
        "create table fake_check (x integer);",
        "drop table fake_check;",
    );

    migrant()
        .current_dir(dir.path())
        .args(["apply", "--fake"])
        .assert()
        .success()
        .stdout(contains("(fake)"))
        .stdout(contains("[✓]"));

    // The migration SQL never ran: un-applying for real would fail to drop
    // the table, so fake the down as well and just verify the status flipped.
    migrant()
        .current_dir(dir.path())
        .args(["apply", "--fake", "-d"])
        .assert()
        .success()
        .stdout(contains("[ ]"));
}

// CLIMIG-4: `--force=skip-failures` continues without recording the failed
// migration; bare `--force` (accept-failures) records it.
#[test]
fn force_modes_through_the_cli() {
    let dir = sqlite_project();
    migrant()
        .current_dir(dir.path())
        .arg("setup")
        .assert()
        .success();
    new_migration(
        dir.path(),
        "a-bad",
        "insert into does_not_exist values (1);",
        "select 1;",
    );
    new_migration(
        dir.path(),
        "b-good",
        "create table good_things (x integer);",
        "drop table good_things;",
    );

    migrant()
        .current_dir(dir.path())
        .args(["apply", "--force=skip-failures"])
        .assert()
        .success()
        .stdout(contains("skip-failures"));
    migrant()
        .current_dir(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicates::str::is_match(r"\[ \] \d{14}_a-bad").expect("valid regex"))
        .stdout(predicates::str::is_match(r"\[✓\] \d{14}_b-good").expect("valid regex"));

    // The skip-failures run above left `b-good` applied ahead of the still
    // unapplied (and earlier-defined) `a-bad`, so the next run needs
    // `--allow-out-of-order` to proceed past that state. Bare `--force`
    // then records the still-failing migration as applied.
    migrant()
        .current_dir(dir.path())
        .args(["apply", "--force", "--allow-out-of-order"])
        .assert()
        .success();
    migrant()
        .current_dir(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicates::str::is_match(r"\[✓\] \d{14}_a-bad").expect("valid regex"));
}

// CLIMIG: `apply --no-sync` is accepted and applies migrations normally. On
// sqlite the advisory lock is a no-op, so this proves the flag is wired
// end-to-end (accepted + migrations applied).
#[test]
fn apply_no_sync_applies_migrations() {
    let dir = sqlite_project();
    migrant()
        .current_dir(dir.path())
        .arg("setup")
        .assert()
        .success();
    new_migration(
        dir.path(),
        "first",
        "create table no_sync_a (x integer);",
        "drop table no_sync_a;",
    );
    new_migration(
        dir.path(),
        "second",
        "create table no_sync_b (x integer);",
        "drop table no_sync_b;",
    );

    migrant()
        .current_dir(dir.path())
        .args(["apply", "--no-sync"])
        .assert()
        .success()
        .stdout(contains("Applying[Up]:"));

    migrant()
        .current_dir(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicates::str::is_match(r"\[✓\] \d{14}_first").expect("valid regex"))
        .stdout(predicates::str::is_match(r"\[✓\] \d{14}_second").expect("valid regex"));

    // `redo --no-sync` applies the flag to both the down and up runs.
    migrant()
        .current_dir(dir.path())
        .args(["redo", "--all", "--no-sync"])
        .assert()
        .success()
        .stdout(contains("Applying[Down]:"))
        .stdout(contains("Applying[Up]:"));

    migrant()
        .current_dir(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicates::str::is_match(r"\[✓\] \d{14}_first").expect("valid regex"))
        .stdout(predicates::str::is_match(r"\[✓\] \d{14}_second").expect("valid regex"));
}

// CLIMIG: `apply` with no flags applies every pending migration in a single
// invocation (previously only the next one ran unless `-a/--all` was given).
#[test]
fn apply_default_applies_all_pending() {
    let dir = sqlite_project();
    migrant()
        .current_dir(dir.path())
        .arg("setup")
        .assert()
        .success();
    new_migration(
        dir.path(),
        "one",
        "create table apply_all_a (x integer);",
        "drop table apply_all_a;",
    );
    new_migration(
        dir.path(),
        "two",
        "create table apply_all_b (x integer);",
        "drop table apply_all_b;",
    );
    new_migration(
        dir.path(),
        "three",
        "create table apply_all_c (x integer);",
        "drop table apply_all_c;",
    );

    migrant()
        .current_dir(dir.path())
        .arg("apply")
        .assert()
        .success();

    migrant()
        .current_dir(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicates::str::is_match(r"\[✓\] \d{14}_one").expect("valid regex"))
        .stdout(predicates::str::is_match(r"\[✓\] \d{14}_two").expect("valid regex"))
        .stdout(predicates::str::is_match(r"\[✓\] \d{14}_three").expect("valid regex"));
}

// CLIMIG: `apply --step N` applies at most N migrations and stops early once
// none remain, instead of erroring.
#[test]
fn apply_step_limits_and_stops_early() {
    let dir = sqlite_project();
    migrant()
        .current_dir(dir.path())
        .arg("setup")
        .assert()
        .success();
    new_migration(
        dir.path(),
        "one",
        "create table apply_step_a (x integer);",
        "drop table apply_step_a;",
    );
    new_migration(
        dir.path(),
        "two",
        "create table apply_step_b (x integer);",
        "drop table apply_step_b;",
    );

    // `--step 1` applies exactly one, leaving the other pending.
    migrant()
        .current_dir(dir.path())
        .args(["apply", "--step", "1"])
        .assert()
        .success();
    migrant()
        .current_dir(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicates::str::is_match(r"\[✓\] \d{14}_one").expect("valid regex"))
        .stdout(predicates::str::is_match(r"\[ \] \d{14}_two").expect("valid regex"));

    // `--step 5`, with only one migration left, applies it and stops early
    // instead of erroring once nothing remains.
    migrant()
        .current_dir(dir.path())
        .args(["apply", "--step", "5"])
        .assert()
        .success();
    migrant()
        .current_dir(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicates::str::is_match(r"\[✓\] \d{14}_one").expect("valid regex"))
        .stdout(predicates::str::is_match(r"\[✓\] \d{14}_two").expect("valid regex"));
}

// CLIMIG: `apply --down` without `--step` still reverts a single migration
// by default.
#[test]
fn apply_down_default_reverts_one() {
    let dir = sqlite_project();
    migrant()
        .current_dir(dir.path())
        .arg("setup")
        .assert()
        .success();
    new_migration(
        dir.path(),
        "one",
        "create table apply_down_a (x integer);",
        "drop table apply_down_a;",
    );
    new_migration(
        dir.path(),
        "two",
        "create table apply_down_b (x integer);",
        "drop table apply_down_b;",
    );

    migrant()
        .current_dir(dir.path())
        .arg("apply")
        .assert()
        .success();

    migrant()
        .current_dir(dir.path())
        .args(["apply", "--down"])
        .assert()
        .success();

    migrant()
        .current_dir(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicates::str::is_match(r"\[✓\] \d{14}_one").expect("valid regex"))
        .stdout(predicates::str::is_match(r"\[ \] \d{14}_two").expect("valid regex"));
}

// CLIMIG-4/DRIFT-6: `--allow-unknown-tags` / `--allow-out-of-order` /
// `--allow-checksum-mismatch` are accepted by both `apply` and `redo`. The full
// scenarios are covered in the library's own test suite; here we only prove the
// CLI wires the flags through without rejecting them.
#[test]
fn allow_flags_are_accepted_by_apply_and_redo() {
    let dir = sqlite_project();
    migrant()
        .current_dir(dir.path())
        .arg("setup")
        .assert()
        .success();
    new_migration(
        dir.path(),
        "one",
        "create table allow_flags_a (x integer);",
        "drop table allow_flags_a;",
    );

    migrant()
        .current_dir(dir.path())
        .args([
            "apply",
            "--allow-unknown-tags",
            "--allow-out-of-order",
            "--allow-checksum-mismatch",
        ])
        .assert()
        .success();

    migrant()
        .current_dir(dir.path())
        .args([
            "redo",
            "--allow-unknown-tags",
            "--allow-out-of-order",
            "--allow-checksum-mismatch",
        ])
        .assert()
        .success();
}

// CLIMIG: `--all`/`-a` is no longer accepted by `apply` (only `redo` keeps
// it).
#[test]
fn apply_rejects_all_flag_but_redo_still_accepts_it() {
    let dir = sqlite_project();
    migrant()
        .current_dir(dir.path())
        .arg("setup")
        .assert()
        .success();

    migrant()
        .current_dir(dir.path())
        .args(["apply", "--all"])
        .assert()
        .failure()
        .stderr(contains("--all"));

    migrant()
        .current_dir(dir.path())
        .args(["apply", "-a"])
        .assert()
        .failure();

    migrant()
        .current_dir(dir.path())
        .args(["redo", "--all"])
        .assert()
        .success();
}
