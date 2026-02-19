use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;

#[test]
fn flow_doctor_example_succeeds() {
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.args([
        "flow",
        "doctor",
        "tests/fixtures/hello-pack/hello-flow.ygtc",
        "--json",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"ok\":true").or(contains("\"status\":\"Ok\"")));
}

#[test]
fn component_doctor_example_reports_expected_failures() {
    let wasm = "fixtures/components/dev.greentic.echo/component.wasm";
    let manifest = "fixtures/components/dev.greentic.echo/component.manifest.json";
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.args(["component", "doctor", wasm, "--manifest", manifest]);
    cmd.assert()
        .failure()
        .stderr(contains("doctor checks failed"))
        .stdout(contains("missing export interface component-descriptor"));
}
