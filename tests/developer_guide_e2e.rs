use assert_cmd::cargo::cargo_bin_cmd;
use greentic_dev::pack_build::{self, PackSigning};
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn developer_guide_happy_path() {
    // Keep temp artifacts inside the workspace so path safety checks pass.
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_dir = workspace.join("target");
    let _ = fs::create_dir_all(&target_dir);
    let tmp = tempfile::Builder::new()
        .prefix("developer-guide-")
        .tempdir_in(&target_dir)
        .or_else(|_| {
            tempfile::Builder::new()
                .prefix("developer-guide-")
                .tempdir()
        })
        .expect("tempdir");
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
    let runner_cli = write_runner_cli_stub(pack_dir);
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.env("GREENTIC_DEV_BIN_GREENTIC_RUNNER_CLI", &runner_cli)
        .args([
            "pack",
            "run",
            "--pack",
            gtpack.to_string_lossy().as_ref(),
            "--offline",
            "--artifacts",
            pack_dir.join("dist/artifacts").to_string_lossy().as_ref(),
        ])
        .assert()
        .success();
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
