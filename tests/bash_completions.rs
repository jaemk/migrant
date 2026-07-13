//! `self bash-completions` integration tests
//!
//! These run the compiled `migrant` binary but touch no database or
//! project config, so they only need the `integration_tests` feature:
//!
//! ```text
//! cargo test --features integration_tests
//! ```
#![cfg(feature = "integration_tests")]

use assert_cmd::Command;
use predicates::str::contains;

fn migrant() -> Command {
    Command::cargo_bin("migrant").expect("binary built")
}

// BASHCO-1
#[test]
fn bash_completions_write_to_stdout() {
    migrant()
        .args(["self", "bash-completions"])
        .assert()
        .success()
        .stdout(contains("_migrant()"))
        .stdout(contains("complete -F _migrant"));
}

// BASHCO-2
#[test]
fn bash_completions_install_writes_to_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("migrant-completions.bash");

    migrant()
        .args(["self", "bash-completions", "install", "--path"])
        .arg(&path)
        .write_stdin("y\n")
        .assert()
        .success()
        .stdout(contains("Completion file will be installed at:"));

    let script = std::fs::read_to_string(&path).unwrap();
    assert!(script.contains("complete -F _migrant"));
}

// BASHCO-2
#[test]
fn bash_completions_install_declined_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("migrant-completions.bash");

    migrant()
        .args(["self", "bash-completions", "install", "--path"])
        .arg(&path)
        .write_stdin("n\n")
        .assert()
        .failure()
        .stderr(contains("Unable to confirm"));

    assert!(!path.exists());
}
