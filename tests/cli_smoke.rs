use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn cli_help_exits_zero() {
    Command::cargo_bin("tidymedia")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("Usage"));
}

#[test]
fn cli_version_exits_zero() {
    Command::cargo_bin("tidymedia")
        .unwrap()
        .arg("--version")
        .assert()
        .success();
}

#[test]
fn cli_find_on_tests_data_succeeds() {
    Command::cargo_bin("tidymedia")
        .unwrap()
        .args(["find", "tests/data"])
        .assert()
        .success();
}

#[test]
fn cli_unknown_subcommand_fails() {
    Command::cargo_bin("tidymedia")
        .unwrap()
        .arg("definitely-not-a-subcommand")
        .assert()
        .failure();
}
