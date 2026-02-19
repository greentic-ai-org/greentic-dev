use assert_cmd::cargo::cargo_bin_cmd;
use predicates::str::contains;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

fn fixture_component() -> (PathBuf, PathBuf) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = root
        .join("fixtures")
        .join("components")
        .join("dev.greentic.echo");
    (
        dir.join("component.wasm"),
        dir.join("component.manifest.json"),
    )
}

#[test]
fn doctor_reports_export_failures_for_legacy_fixture() {
    let (wasm, _) = fixture_component();
    cargo_bin_cmd!("greentic-component")
        .args(["doctor", wasm.to_str().expect("utf-8 path")])
        .assert()
        .failure()
        .stderr(contains("doctor checks failed"))
        .stdout(contains("missing export interface component-descriptor"));
}

#[test]
fn doctor_fails_without_manifest_when_separated() {
    let (wasm, manifest) = fixture_component();
    let temp = tempdir().expect("tempdir");
    let relocated_wasm = temp.path().join("component.wasm");
    fs::copy(&wasm, &relocated_wasm).expect("copy wasm");

    cargo_bin_cmd!("greentic-component")
        .args(["doctor", relocated_wasm.to_str().expect("utf-8 path")])
        .assert()
        .failure()
        .stderr(contains("doctor checks failed"))
        .stdout(contains("missing export interface component-descriptor"));

    cargo_bin_cmd!("greentic-component")
        .args([
            "doctor",
            relocated_wasm.to_str().expect("utf-8 path"),
            "--manifest",
            manifest.to_str().expect("utf-8 path"),
        ])
        .assert()
        .failure()
        .stderr(contains("doctor checks failed"))
        .stdout(contains("missing export interface component-descriptor"));
}
