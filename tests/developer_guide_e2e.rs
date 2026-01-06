use greentic_dev::pack_build::{self, PackSigning};
use greentic_dev::pack_run::{self, MockSetting, PackRunConfig, RunPolicy};
use serde_json::json;
use std::fs;
use std::path::PathBuf;

#[test]
fn developer_guide_happy_path() {
    // Keep temp artifacts inside the workspace so path safety checks pass.
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let tmp = tempfile::tempdir_in(workspace.join("target")).expect("tempdir");
    let pack_dir = tmp.path();

    // Minimal flow that exercises component.exec with the dev.greentic.echo fixture component.
    fs::create_dir_all(pack_dir.join("flows")).expect("flows dir");
    let flow_path = pack_dir.join("flows/main.ygtc");
    let starter_flow = r#"id: main
type: messaging
title: Welcome
description: Minimal starter flow
start: start

nodes:
  start:
    component.exec:
      component: dev.greentic.echo
      operation: echo
      input:
        message: "Hello from greentic-dev developer guide test!"
    routing:
      - out: true
"#;
    fs::write(&flow_path, starter_flow).expect("write starter flow");

    // Build the pack using local fixtures/components for resolution.
    let gtpack = pack_dir.join("dist/hello.gtpack");
    fs::create_dir_all(gtpack.parent().unwrap()).expect("create dist dir");
    pack_build::run(
        &flow_path,
        &gtpack,
        PackSigning::Dev,
        None,
        Some(&workspace.join("fixtures/components")),
    )
    .expect("pack build");

    // Execute the pack offline with mocks to verify the runtime path.
    pack_run::run(PackRunConfig {
        pack_path: &gtpack,
        entry: None,
        input: Some(json!({}).to_string()),
        policy: RunPolicy::DevOk,
        otlp: None,
        allow_hosts: None,
        mocks: MockSetting::On,
        artifacts_dir: Some(pack_dir.join("dist/artifacts").as_path()),
        json: false,
        offline: true,
        mock_exec: false,
        allow_external: false,
        mock_external: false,
        mock_external_payload: None,
        secrets_seed: None,
    })
    .expect("pack run");
}
