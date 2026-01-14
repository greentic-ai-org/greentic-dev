use assert_cmd::cargo::cargo_bin_cmd;
use greentic_dev::{
    pack_build::{self, PackSigning},
    pack_verify::{self, VerifyPolicy},
};
use std::fs;
use std::path::{Path, PathBuf};

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

    let runner_cli = write_runner_cli_stub(temp.path());
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.env("GREENTIC_DEV_BIN_GREENTIC_RUNNER_CLI", &runner_cli)
        .args([
            "pack",
            "run",
            "--pack",
            pack_path.to_string_lossy().as_ref(),
            "--artifacts",
            artifacts_dir.to_string_lossy().as_ref(),
        ])
        .assert()
        .success();

    pack_verify::run(&pack_path, VerifyPolicy::DevOk, false).expect("pack verify");
}

fn write_runner_cli_stub(dir: &Path) -> PathBuf {
    #[cfg(windows)]
    let path = dir.join("greentic-runner-cli.cmd");
    #[cfg(not(windows))]
    let path = dir.join("greentic-runner-cli");

    #[cfg(windows)]
    let script = "@echo stub runner\r\n";
    #[cfg(not(windows))]
    let script = "#!/bin/sh\necho \"stub runner\"\n";

    fs::write(&path, script).expect("write stub");

    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path).expect("stub metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).expect("set stub permissions");
    }

    path
}
