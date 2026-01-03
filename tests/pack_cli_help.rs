use assert_cmd::cargo::cargo_bin_cmd;
use predicates::str::contains;

#[test]
fn pack_help_lists_new_subcommands() {
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.args(["pack", "--help"])
        .assert()
        .success()
        .stdout(contains("components"))
        .stdout(contains("update"))
        .stdout(contains("config"))
        .stdout(contains("gui"));
}

#[test]
fn pack_components_help_succeeds() {
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.args(["pack", "components", "--help"])
        .assert()
        .success();
}

#[test]
fn pack_update_help_succeeds() {
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.args(["pack", "update", "--help"]).assert().success();
}

#[test]
fn pack_config_help_succeeds() {
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.args(["pack", "config", "--help"]).assert().success();
}

#[test]
fn pack_gui_help_succeeds() {
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.args(["pack", "gui", "--help"]).assert().success();
}
