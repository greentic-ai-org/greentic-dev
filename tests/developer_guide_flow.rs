mod support;

use anyhow::{Result, anyhow};
use assert_cmd::Command;
use serde_json::json;
use serde_yaml_bw as yaml;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

fn resolve_bin() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_greentic-dev") {
        return Ok(PathBuf::from(path));
    }
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_greentic_dev") {
        return Ok(PathBuf::from(path));
    }
    let current = std::env::current_exe()?;
    let candidate = current
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("greentic-dev"));
    match candidate {
        Some(path) if path.exists() => Ok(path),
        _ => anyhow::bail!("unable to locate greentic-dev binary for tests"),
    }
}

fn component_wasm(component_dir: &Path) -> Result<PathBuf> {
    let target = component_dir
        .join("target")
        .join("wasm32-wasip2")
        .join("release");
    let wasm = fs::read_dir(&target)?
        .filter_map(|entry| entry.ok())
        .map(|e| e.path())
        .find(|p| p.extension().map(|ext| ext == "wasm").unwrap_or(false))
        .ok_or_else(|| anyhow!("component wasm not found in {}", target.display()))?;
    Ok(wasm)
}

fn ystr(s: &str) -> yaml::Value {
    yaml::Value::String(s.to_string(), None)
}

fn yseq(values: Vec<yaml::Value>) -> yaml::Value {
    yaml::Value::Sequence(yaml::Sequence {
        anchor: None,
        elements: values,
    })
}

#[test]
fn developer_guide_end_to_end_flow() -> Result<()> {
    if std::env::var("GREENTIC_ALLOW_NETWORK_TESTS").as_deref() != Ok("1") {
        eprintln!("skipping developer_guide_end_to_end_flow (networked build not allowed)");
        return Ok(());
    }
    let bin = resolve_bin()?;
    let tmp = tempdir()?;
    let root = tmp.path();

    // Scaffold pack
    let pack_dir = root.join("hello-pack");
    Command::new(&bin)
        .args([
            "pack",
            "new",
            "--",
            "--dir",
            pack_dir.to_str().expect("pack path utf-8"),
            "dev.local.hello-pack",
        ])
        .assert()
        .success();

    // Scaffold component inside pack/components/hello-world
    let component_dir = pack_dir.join("components/hello-world");
    Command::new(&bin)
        .current_dir(&pack_dir)
        .args([
            "component",
            "new",
            "--name",
            "hello-world",
            "--path",
            component_dir.to_str().expect("component path utf-8"),
            "--non-interactive",
            "--no-git",
            "--no-check",
        ])
        .assert()
        .success();

    let manifest_path = component_dir.join("component.manifest.json");

    // Build component (regenerates flows/config schema)
    Command::new(&bin)
        .args([
            "component",
            "build",
            "--manifest",
            manifest_path.to_str().expect("manifest path utf-8"),
        ])
        .assert()
        .success();

    // Ensure dev_flows.default exists for flow add-step.
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    if manifest
        .get("dev_flows")
        .and_then(|v| v.get("default"))
        .is_none()
    {
        manifest
            .as_object_mut()
            .expect("manifest object")
            .entry("dev_flows")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .expect("dev_flows object")
            .insert(
                "default".to_string(),
                json!({
                    "format": "flow-ir-json",
                    "graph": {
                        "schema_version": 1,
                        "id": "component.default",
                        "type": "component-config",
                        "nodes": {
                            "emit_config": {
                                "template": "{ \"node_id\": \"hello\", \"node\": { \"qa\": { \"component\": \"component-qa-process\", \"question\": \"hi\" }, \"routing\": [{ \"to\": \"NEXT_NODE_PLACEHOLDER\" }] } }"
                            }
                        },
                        "edges": []
                    }
                }),
            );
        fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;
    }

    let wasm_path = component_wasm(&component_dir)?;

    // Doctor the built component (manifest is not colocated with the wasm).
    Command::new(&bin)
        .args([
            "component",
            "doctor",
            wasm_path.to_str().expect("wasm path utf-8"),
            "--manifest",
            manifest_path.to_str().expect("manifest path utf-8"),
        ])
        .assert()
        .success();

    // Scaffold pack
    let pack_dir = root.join("hello-pack");
    Command::new(&bin)
        .args([
            "pack",
            "new",
            "--",
            "--dir",
            pack_dir.to_str().expect("pack path utf-8"),
            "dev.local.hello-pack",
        ])
        .assert()
        .success();

    // Normalize the starter flow to a simple pack flow (start -> end) with id hello.
    let starter_flow = "schema_version: 1
id: hello
type: pack
nodes:
  start:
    routing:
      - to: end
  end: {}
";
    fs::write(pack_dir.join("flows/hello.ygtc"), starter_flow)?;

    // Wire the component into the pack flow via config flow.
    Command::new(&bin)
        .current_dir(&pack_dir)
        .args([
            "flow",
            "add-step",
            "main",
            "--manifest",
            manifest_path.to_str().expect("manifest path utf-8"),
            "--coordinate",
            component_dir.to_str().expect("component path utf-8"),
            "--after",
            "start",
        ])
        .assert()
        .success();

    // Update pack.yaml to point at the built component artifact.
    let pack_yaml_path = pack_dir.join("pack.yaml");
    let mut pack_yaml: serde_yaml_bw::Value =
        serde_yaml_bw::from_str(&fs::read_to_string(&pack_yaml_path)?)?;
    if let Some(components) = pack_yaml
        .as_mapping_mut()
        .and_then(|m| m.get_mut(ystr("components")))
        .and_then(|v| v.as_sequence_mut())
        .and_then(|seq| seq.elements.get_mut(0))
        .and_then(|entry| entry.as_mapping_mut())
    {
        components.insert(
            ystr("id"),
            ystr(
                manifest
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("dev.local.component"),
            ),
        );
        components.insert(
            ystr("version"),
            ystr(
                manifest
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0.1.0"),
            ),
        );
        components.insert(
            ystr("world"),
            ystr(
                manifest
                    .get("world")
                    .and_then(|v| v.as_str())
                    .unwrap_or("greentic:component/component@0.4.0"),
            ),
        );
        components.insert(ystr("supports"), yseq(vec![ystr("messaging")]));
        // Copy the component wasm into the pack workspace for a stable relative path.
        let component_dest = pack_dir.join("components").join("component.wasm");
        fs::create_dir_all(component_dest.parent().expect("components dir"))?;
        fs::copy(&wasm_path, &component_dest)?;
        let wasm_rel = component_dest
            .strip_prefix(&pack_dir)
            .unwrap_or(&component_dest)
            .display()
            .to_string();
        components.insert(ystr("wasm"), ystr(&wasm_rel));
    }
    fs::write(&pack_yaml_path, yaml::to_string(&pack_yaml)?)?;

    // Build the pack
    let gtpack = pack_dir.join("dist/hello.gtpack");
    Command::new(&bin)
        .args([
            "pack",
            "build",
            "--",
            "--in",
            pack_dir.to_str().expect("pack path utf-8"),
            "--gtpack-out",
            gtpack.to_str().expect("gtpack path utf-8"),
        ])
        .assert()
        .success();

    // Run the pack offline with mocks.
    Command::new(&bin)
        .args([
            "pack",
            "run",
            "--pack",
            gtpack.to_str().expect("gtpack path utf-8"),
            "--offline",
            "--mocks",
            "on",
        ])
        .assert()
        .success();

    Ok(())
}
