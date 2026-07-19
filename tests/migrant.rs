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
use predicates::str::contains;

fn migrant() -> Command {
    Command::cargo_bin("migrant").expect("binary built")
}

#[test]
fn kitchen_sink() {
    // make sure we're setup and back to no applied migrations
    migrant().arg("setup").assert().success();
    let _ = migrant().args(["apply", "-ad"]).assert();

    // A down run with nothing left to un-apply is not an error; it succeeds and
    // reports the (all-unapplied) status.
    migrant()
        .args(["apply", "-ad"])
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

    migrant()
        .args(["apply", "-a"])
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

    let _ = migrant().args(["apply", "-ad"]).assert();
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

    // apply only the first migration so we have one applied, one pending
    migrant()
        .current_dir(dir.path())
        .arg("apply")
        .assert()
        .success();

    // default (text) format: summary line plus a marked row per migration
    migrant()
        .current_dir(dir.path())
        .arg("status")
        .assert()
        .success()
        .stdout(contains("Migration status: 1 applied, 1 pending (2 total)"))
        .stdout(predicates::str::is_match(r"\[✓\] \d{14}_first").expect("valid regex"))
        .stdout(predicates::str::is_match(r"\[ \] \d{14}_second").expect("valid regex"));

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
    assert_eq!(value["migrations"].as_array().expect("array").len(), 2);
    assert_eq!(value["migrations"][0]["applied"], true);
    assert_eq!(value["migrations"][1]["applied"], false);
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
        .args(["apply", "--all", "--force=skip-failures"])
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

    // Bare `--force` records the still-failing migration as applied.
    migrant()
        .current_dir(dir.path())
        .args(["apply", "--all", "--force"])
        .assert()
        .success();
    migrant()
        .current_dir(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicates::str::is_match(r"\[✓\] \d{14}_a-bad").expect("valid regex"));
}

// CLIMIG: `apply --all --no-sync` is accepted and applies migrations
// normally. On sqlite the advisory lock is a no-op, so this proves the flag
// is wired end-to-end (accepted + migrations applied).
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
        .args(["apply", "--all", "--no-sync"])
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
