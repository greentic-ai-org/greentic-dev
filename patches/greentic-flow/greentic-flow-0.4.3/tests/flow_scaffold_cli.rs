use assert_cmd::prelude::*;
use serde::Deserialize;
use std::{fs, process::Command};
use tempfile::tempdir;

#[derive(Deserialize)]
struct FlowEntry {
    id: String,
    file: String,
}

#[derive(Deserialize)]
struct ManifestDoc {
    flows: Vec<FlowEntry>,
}

#[test]
fn scaffolds_deployment_flow() {
    let dir = tempdir().unwrap();
    let flow_path = dir.path().join("deploy_flow.ygtc");

    Command::new(assert_cmd::cargo::cargo_bin!("greentic-flow"))
        .arg("new")
        .arg(flow_path.as_os_str())
        .arg("--kind")
        .arg("deployment")
        .assert()
        .success();

    let content = fs::read_to_string(&flow_path).unwrap();
    assert!(content.contains("type: events"));
    assert!(content.contains("deploy.renderer"));
    assert!(content.contains("DeploymentPlan"));
}

#[test]
fn scaffolds_messaging_flow() {
    let dir = tempdir().unwrap();
    let flow_path = dir.path().join("chat_flow.ygtc");

    Command::new(assert_cmd::cargo::cargo_bin!("greentic-flow"))
        .arg("new")
        .arg(flow_path.as_os_str())
        .assert()
        .success();

    let content = fs::read_to_string(&flow_path).unwrap();
    assert!(content.contains("type: messaging"));
    assert!(content.contains("component.kind.entry"));
}

#[test]
fn warns_when_deployment_pack_creates_messaging_flow() {
    let dir = tempdir().unwrap();
    let manifest_path = dir.path().join("manifest.yaml");
    fs::write(&manifest_path, "kind: deployment\n").unwrap();
    let flow_path = dir.path().join("flow.ygtc");

    let assert = Command::new(assert_cmd::cargo::cargo_bin!("greentic-flow"))
        .arg("new")
        .arg(flow_path.as_os_str())
        .arg("--kind")
        .arg("messaging")
        .arg("--pack-manifest")
        .arg(manifest_path.as_os_str())
        .assert()
        .success();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("pack is marked kind: deployment"),
        "stderr did not contain info note: {stderr}"
    );
}

#[test]
fn defaults_to_deployment_kind_from_manifest() {
    let dir = tempdir().unwrap();
    let manifest_path = dir.path().join("manifest.yaml");
    fs::write(
        &manifest_path,
        r#"id: demo.pack
kind: deployment
flows: []
"#,
    )
    .unwrap();
    let flow_path = dir.path().join("deploy_flow.ygtc");

    Command::new(assert_cmd::cargo::cargo_bin!("greentic-flow"))
        .arg("new")
        .arg(flow_path.as_os_str())
        .arg("--pack-manifest")
        .arg(manifest_path.as_os_str())
        .assert()
        .success();

    let content = fs::read_to_string(&flow_path).unwrap();
    assert!(content.contains("type: events"));
}

#[test]
fn registers_flow_in_manifest() {
    let dir = tempdir().unwrap();
    let manifest_path = dir.path().join("manifest.yaml");
    fs::write(
        &manifest_path,
        r#"id: demo.pack
flows:
  - id: existing
    file: flows/existing.ygtc
"#,
    )
    .unwrap();
    let flows_dir = dir.path().join("flows");
    fs::create_dir_all(&flows_dir).unwrap();
    let flow_path = flows_dir.join("new_flow.ygtc");

    Command::new(assert_cmd::cargo::cargo_bin!("greentic-flow"))
        .arg("new")
        .arg(flow_path.as_os_str())
        .arg("--pack-manifest")
        .arg(manifest_path.as_os_str())
        .assert()
        .success();

    let manifest: ManifestDoc =
        serde_yaml_bw::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    assert_eq!(manifest.flows.len(), 2);
    assert!(
        manifest
            .flows
            .iter()
            .any(|entry| entry.id == "new_flow" && entry.file == "flows/new_flow.ygtc")
    );
}
