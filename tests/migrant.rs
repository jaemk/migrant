extern crate assert_cli;

use assert_cli::Assert;


#[test]
fn kitchen_sink() {
    // make sure we're setup and back to no applied migrations
    Assert::main_binary()
        .with_args(&["setup"])
        .unwrap();
    Assert::main_binary()
        .with_args(&["apply", "-ad"])
        .execute().is_ok();

    Assert::main_binary()
        .with_args(&["apply", "-ad"])
        .fails()
        .stderr().contains("MigrationComplete: No un-applied `Down` migrations found")
        .unwrap();

    Assert::main_binary()
        .with_args(&["list"])
        .stdout().contains("Current Migration Status:")
        .stdout().contains("[ ] 20170812145327_initial")
        .stdout().contains("[ ] 20171126194042_second")
        .unwrap();

    Assert::main_binary()
        .with_args(&["apply", "-a"])
        .stdout().contains("Applying[Up]:")
        .stdout().contains("Current Migration Status:")
        .stdout().contains("[✓] 20170812145327_initial")
        .stdout().contains("[✓] 20171126194042_second")
        .unwrap();

    Assert::main_binary()
        .with_args(&["list"])
        .stdout().contains("Current Migration Status:")
        .stdout().contains("[✓] 20170812145327_initial")
        .stdout().contains("[✓] 20171126194042_second")
        .unwrap();

    Assert::main_binary()
        .with_args(&["redo"])
        .stdout().contains("Applying[Down]:")
        .stdout().contains("Current Migration Status:")
        .stdout().contains("[ ] 20171126194042_second")
        .stdout().contains("Applying[Up]:")
        .stdout().contains("Current Migration Status:")
        .stdout().contains("[✓] 20171126194042_second")
        .unwrap();

    Assert::main_binary()
        .with_args(&["redo", "--all"])
        .stdout().contains("Applying[Down]:")
        .stdout().contains("Current Migration Status:")
        .stdout().contains("[ ] 20170812145327_initial")
        .stdout().contains("[ ] 20171126194042_second")
        .stdout().contains("Applying[Up]:")
        .stdout().contains("Current Migration Status:")
        .stdout().contains("[✓] 20170812145327_initial")
        .stdout().contains("[✓] 20171126194042_second")
        .unwrap();
}

