use std::path::PathBuf;

use assert_cmd::cargo::cargo_bin_cmd;
use greentic_dev::pack_build::{self, PackSigning};
use predicates::prelude::*;

#[test]
fn pack_run_respects_component_exec_operation() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let flow_path = root.join("tests/fixtures/hello-pack/hello-flow.ygtc");
    let component_dir = root.join("fixtures/components");

    let temp = tempfile::tempdir().expect("tempdir");
    let pack_path = temp.path().join("component-exec.gtpack");
    let artifacts_dir = temp.path().join("artifacts");

    // Build a pack that uses component.exec with an explicit operation.
    pack_build::run(
        &flow_path,
        &pack_path,
        PackSigning::Dev,
        None,
        Some(component_dir.as_path()),
    )
    .expect("pack build");

    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.env("GREENTIC_DEV_OFFLINE", "1")
        .current_dir(&root)
        .arg("pack")
        .arg("run")
        .arg("--pack")
        .arg(&pack_path)
        .arg("--offline")
        .arg("--mocks")
        .arg("on")
        .arg("--artifacts")
        .arg(&artifacts_dir)
        .arg("--json");

    cmd.assert()
        .success()
        .stderr(predicate::str::contains("requires an operation").not())
        .stdout(predicate::str::contains("\"status\":\"Success\""));
}
