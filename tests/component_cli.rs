use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use serde_json::Value;
use tempfile::TempDir;
use which::which;

const TEMPLATE_ID: &str = "rust-wasi-p2-min";
const DEFAULT_ORG: &str = "ai.greentic";

#[test]
fn component_templates_json_smoke() -> Result<()> {
    if skip_component_tool() {
        return Ok(());
    }
    let cli = cli_bin()?;
    let output = Command::new(&cli)
        .args(["component", "templates", "--json"])
        .output()
        .with_context(|| format!("failed to spawn {}", cli.display()))?;
    if !output.status.success() {
        panic!(
            "`greentic-dev component templates --json` failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let value: Value = serde_json::from_slice(&output.stdout)
        .context("templates command did not emit valid JSON")?;
    assert_eq!(value["tool"], "greentic-component");
    assert_eq!(value["command"], "templates");
    assert!(value["templates"].is_array(), "templates key missing");
    Ok(())
}

#[test]
fn component_new_scaffold_reports_sections() -> Result<()> {
    if skip_component_tool() {
        return Ok(());
    }
    let temp = TempDir::new()?;
    let path = temp.path().join("component-json-demo");
    let cli = cli_bin()?;
    let args = build_new_args(&path);
    let output = Command::new(&cli)
        .args(args)
        .output()
        .with_context(|| format!("failed to spawn {}", cli.display()))?;
    if !output.status.success() {
        panic!(
            "`greentic-dev component new --json` failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let value: Value =
        serde_json::from_slice(&output.stdout).context("component new did not emit JSON")?;
    assert_eq!(value["tool"], "greentic-component");
    assert_eq!(value["command"], "new");
    assert!(value["scaffold"].is_object(), "scaffold section missing");
    assert!(
        value
            .get("compile_check")
            .map(|v| v.is_object())
            .unwrap_or(false),
        "compile_check section missing"
    );
    assert!(path.exists(), "scaffold path was not created");
    Ok(())
}

#[test]
fn component_doctor_succeeds_on_scaffold() -> Result<()> {
    if skip_component_tool() {
        return Ok(());
    }
    let temp = TempDir::new()?;
    let path = temp.path().join("component-doc-demo");
    scaffold_component(&path)?;
    let cli = cli_bin()?;
    let status = Command::new(&cli)
        .args(["component", "doctor", "--path", path.to_str().unwrap()])
        .status()
        .with_context(|| format!("failed to spawn {}", cli.display()))?;
    assert!(status.success(), "component doctor failed");
    Ok(())
}

fn scaffold_component(path: &Path) -> Result<()> {
    let cli = cli_bin()?;
    let args = build_new_args(path);
    let status = Command::new(&cli)
        .args(args)
        .status()
        .with_context(|| format!("failed to spawn {}", cli.display()))?;
    if !status.success() {
        anyhow::bail!("component new scaffold failed");
    }
    Ok(())
}

fn build_new_args(path: &Path) -> Vec<String> {
    vec![
        "component".into(),
        "new".into(),
        "--name".into(),
        path.file_name().unwrap().to_string_lossy().to_string(),
        "--path".into(),
        path.to_string_lossy().to_string(),
        "--org".into(),
        DEFAULT_ORG.into(),
        "--template".into(),
        TEMPLATE_ID.into(),
        "--license".into(),
        "Apache-2.0".into(),
        "--non-interactive".into(),
        "--json".into(),
    ]
}

fn cli_bin() -> Result<PathBuf> {
    const ENV_KEYS: [&str; 2] = ["CARGO_BIN_EXE_greentic-dev", "CARGO_BIN_EXE_greentic_dev"];
    for key in ENV_KEYS {
        if let Ok(path) = env::var(key) {
            return Ok(PathBuf::from(path));
        }
    }
    anyhow::bail!(
        "cargo did not export any of {ENV_KEYS:?}; ensure greentic-dev binary is built with the \
         tests"
    );
}

fn skip_component_tool() -> bool {
    if which("greentic-component").is_err() {
        eprintln!("skipping component CLI integration tests (greentic-component missing)");
        return true;
    }
    false
}
