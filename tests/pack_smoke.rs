use std::path::PathBuf;

use greentic_dev::{
    pack_build::{self, PackSigning},
    pack_run::{self, MockSetting, PackRunConfig, RunPolicy},
    pack_verify::{self, VerifyPolicy},
};

#[test]
fn pack_build_run_verify_smoke() {
    // This uses the workspace-built runner/host (with local [patch.crates-io] overrides)
    // so the flow/component ids from the fixture pack are preserved end-to-end.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let flow_path = root.join("tests/fixtures/hello-pack/hello-flow.ygtc");
    let component_dir = root.join("fixtures/components");

    let temp = tempfile::tempdir().expect("tempdir");
    let pack_path = temp.path().join("smoke.gtpack");
    let artifacts_dir = temp.path().join("artifacts");
    pack_build::run(
        &flow_path,
        &pack_path,
        PackSigning::Dev,
        None,
        Some(component_dir.as_path()),
    )
    .expect("pack build");

    pack_run::run(PackRunConfig {
        pack_path: &pack_path,
        entry: None,
        input: None,
        policy: RunPolicy::DevOk,
        otlp: None,
        allow_hosts: None,
        mocks: MockSetting::Off,
        artifacts_dir: Some(artifacts_dir.as_path()),
        json: false,
        offline: false,
        mock_exec: false,
        allow_external: false,
        mock_external: false,
        mock_external_payload: None,
        secrets_seed: None,
    })
    .expect("pack run");

    pack_verify::run(&pack_path, VerifyPolicy::DevOk, false).expect("pack verify");
}
