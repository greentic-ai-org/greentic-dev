use assert_cmd::cargo::cargo_bin_cmd;
use greentic_dev::pack_build::{self, PackSigning};
use greentic_types::decode_pack_manifest;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use zip::ZipArchive;

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

#[test]
fn developer_guide_hello2_remote_templates_pack_run() {
    // Mirrors the developer-guide "hello2-pack" example using an OCI component reference.
    // TODO: This currently fails because the runner cannot resolve the OCI component from the pack.
    let temp = tempfile::tempdir().expect("tempdir");
    let pack_dir = temp.path().join("hello2-pack");
    fs::create_dir_all(pack_dir.join("flows")).expect("flows dir");

    let pack_yaml = r#"pack_id: dev.local.hello2-pack
version: 0.1.0
kind: application
publisher: Greentic

components: []

flows:
  - id: main
    file: flows/main.ygtc
    tags: [default]
    entrypoints: [default]

dependencies: []

assets: []

extensions:
  greentic.components:
    kind: greentic.components
    version: v1
    inline:
      refs:
        - ghcr.io/greentic-ai/components/templates:latest
"#;
    fs::write(pack_dir.join("pack.yaml"), pack_yaml).expect("write pack.yaml");

    let flow_yaml = r#"id: main
type: messaging
start: templates
nodes:
  templates:
    handle_message:
      input:
        input: "Hello from templates!"
    routing:
      - out: true
"#;
    fs::write(pack_dir.join("flows/main.ygtc"), flow_yaml).expect("write flow");

    let resolve_json = r#"{
  "schema_version": 1,
  "flow": "main.ygtc",
  "nodes": {
    "templates": {
      "source": {
        "kind": "oci",
        "ref": "oci://ghcr.io/greentic-ai/components/templates:latest"
      }
    }
  }
}
"#;
    fs::write(pack_dir.join("flows/main.ygtc.resolve.json"), resolve_json)
        .expect("write resolve sidecar");

    let digest_hex = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let digest = format!("sha256:{digest_hex}");
    let summary_json = format!(
        r#"{{
  "schema_version": 1,
  "flow": "main.ygtc",
  "nodes": {{
    "templates": {{
      "component_id": "ai.greentic.component-templates",
      "source": {{
        "kind": "oci",
        "ref": "oci://ghcr.io/greentic-ai/components/templates:latest"
      }},
      "digest": "{digest}"
    }}
  }}
}}
"#
    );
    fs::write(
        pack_dir.join("flows/main.ygtc.resolve.summary.json"),
        summary_json,
    )
    .expect("write resolve summary");

    let cache_root = temp.path().join("cache");
    let cache_dir = cache_root.join(digest_hex);
    fs::create_dir_all(&cache_dir).expect("create cache dir");
    fs::write(cache_dir.join("component.wasm"), b"\0asm\x01\0\0\0")
        .expect("write cached component");

    let gtpack_path = pack_dir.join("dist/hello2.gtpack");
    fs::create_dir_all(gtpack_path.parent().unwrap()).expect("create dist dir");

    let build_status = std::process::Command::new("greentic-pack")
        .args([
            "--cache-dir",
            cache_root.to_string_lossy().as_ref(),
            "build",
            "--in",
            pack_dir.to_str().unwrap(),
            "--gtpack-out",
            gtpack_path.to_str().unwrap(),
            "--allow-oci-tags",
        ])
        .status()
        .expect("failed to spawn greentic-pack build");
    assert!(build_status.success(), "greentic-pack build failed");

    eprintln!("{}", inspect_pack_manifest(&gtpack_path));

    let artifacts_dir = pack_dir.join("dist/artifacts");
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.args([
        "pack",
        "run",
        "--pack",
        gtpack_path.to_string_lossy().as_ref(),
        "--artifacts",
        artifacts_dir.to_string_lossy().as_ref(),
    ])
    .assert()
    .success();
}

fn inspect_pack_manifest(gtpack_path: &Path) -> String {
    let file = match fs::File::open(gtpack_path) {
        Ok(file) => file,
        Err(err) => return format!("manifest inspect failed: {err}"),
    };
    let mut archive = match ZipArchive::new(file) {
        Ok(archive) => archive,
        Err(err) => return format!("manifest inspect failed: {err}"),
    };
    let mut manifest_bytes = Vec::new();
    let mut file = match archive.by_name("manifest.cbor") {
        Ok(file) => file,
        Err(err) => return format!("manifest inspect failed: {err}"),
    };
    if let Err(err) = file.read_to_end(&mut manifest_bytes) {
        return format!("manifest inspect failed: {err}");
    }
    let manifest = match decode_pack_manifest(&manifest_bytes) {
        Ok(manifest) => manifest,
        Err(err) => return format!("manifest inspect failed: {err}"),
    };
    let ids: Vec<_> = manifest
        .components
        .iter()
        .map(|component| component.id.as_str())
        .collect();
    format!(
        "pack manifest: pack_id={} components={} [{}]",
        manifest.pack_id.as_str(),
        manifest.components.len(),
        ids.join(", ")
    )
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
