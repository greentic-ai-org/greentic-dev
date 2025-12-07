use assert_cmd::prelude::*;
use std::process::Command;

#[test]
fn cli_passes_with_registry() {
    Command::new(assert_cmd::cargo::cargo_bin!("ygtc-lint"))
        .args([
            "--schema",
            "schemas/ygtc.flow.schema.json",
            "--registry",
            "tests/data/registry_ok.json",
            "tests/data/flow_ok.ygtc",
        ])
        .assert()
        .success();
}

#[test]
fn cli_fails_with_missing_adapter() {
    let assert = Command::new(assert_cmd::cargo::cargo_bin!("ygtc-lint"))
        .args([
            "--schema",
            "schemas/ygtc.flow.schema.json",
            "--registry",
            "tests/data/registry_ok.json",
            "tests/data/flow_fail.ygtc",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("adapter_resolvable"));
}
