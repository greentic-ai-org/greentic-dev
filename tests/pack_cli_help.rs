use assert_cmd::cargo::cargo_bin_cmd;
use greentic_dev::passthrough::resolve_binary;
use predicates::str::contains;

fn skip_if_greentic_pack_missing() -> bool {
    if resolve_binary("greentic-pack").is_err() {
        eprintln!("skipping pack help tests: greentic-pack not found");
        true
    } else {
        false
    }
}

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
    if skip_if_greentic_pack_missing() {
        return;
    }
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.args(["pack", "components", "--help"])
        .assert()
        .success();
}

#[test]
fn pack_update_help_succeeds() {
    if skip_if_greentic_pack_missing() {
        return;
    }
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.args(["pack", "update", "--help"]).assert().success();
}

#[test]
fn pack_config_help_succeeds() {
    if skip_if_greentic_pack_missing() {
        return;
    }
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.args(["pack", "config", "--help"]).assert().success();
}

#[test]
fn pack_gui_help_succeeds() {
    if skip_if_greentic_pack_missing() {
        return;
    }
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.args(["pack", "gui", "--help"]).assert().success();
}
