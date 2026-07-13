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

    migrant()
        .args(["apply", "-ad"])
        .assert()
        .failure()
        .stderr(contains(
            "MigrationComplete: No un-applied `Down` migrations found",
        ));

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
