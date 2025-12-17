mod support;

use std::process::Command;

use anyhow::{Context, Result};
use serde_json::Value as JsonValue;
use support::l4::build_l4_pack;
use support::{Workspace, diag_with_owner};

fn resolve_bin() -> Result<std::path::PathBuf> {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_greentic-dev") {
        return Ok(std::path::PathBuf::from(path));
    }
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_greentic_dev") {
        return Ok(std::path::PathBuf::from(path));
    }
    let current = std::env::current_exe().context("current_exe")?;
    let candidate = current
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("greentic-dev"))
        .ok_or_else(|| anyhow::anyhow!("cannot resolve greentic-dev binary"))?;
    Ok(candidate)
}

fn parse_json(stdout: &str) -> Result<JsonValue> {
    serde_json::from_str(stdout.trim()).context("stdout is not valid JSON")
}

fn run_cli(
    pack_path: &std::path::Path,
    input: &str,
    offline: bool,
    allow_external: bool,
    mock_external: bool,
    secrets_seed: Option<&std::path::Path>,
) -> Result<(i32, String, String)> {
    let bin = resolve_bin()?;
    let mut cmd = Command::new(bin);
    cmd.arg("pack")
        .arg("run")
        .arg("-p")
        .arg(pack_path)
        .arg("--json")
        .arg("--mock-exec")
        .arg("--input")
        .arg(input)
        .env("HTTP_PROXY", "")
        .env("HTTPS_PROXY", "")
        .env("ALL_PROXY", "")
        .env("NO_PROXY", "*");
    if offline {
        cmd.arg("--offline");
    } else {
        cmd.env_remove("NO_PROXY");
    }
    if let Some(seed) = secrets_seed {
        cmd.arg("--secrets-seed").arg(seed);
    }
    if allow_external {
        cmd.arg("--allow-external");
    }
    if mock_external {
        cmd.arg("--mock-external");
    }
    let output = cmd.output().context("failed to run CLI")?;
    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok((code, stdout, stderr))
}

#[test]
fn pack_realism_l4_1_cli_external_blocked() -> Result<()> {
    let workspace = Workspace::new("realism-l4.1-blocked")?;
    let pack_bytes = build_l4_pack()?;
    let pack_path = workspace.root.join("l4.gtpack");
    std::fs::write(&pack_path, &pack_bytes)?;

    let (code, stdout, stderr) =
        run_cli(&pack_path, r#"{"query":"hi"}"#, true, false, false, None)?;
    if code == 0 {
        diag_with_owner(
            "pack_realism_l4_1_cli_external_blocked",
            "execute",
            &workspace,
            &format!("expected non-zero exit, got stdout={stdout}, stderr={stderr}"),
            "greentic-dev",
        );
        anyhow::bail!("cli exit code {code}");
    }
    let doc = parse_json(&stdout)?;
    let status = doc.get("status").and_then(|v| v.as_str()).unwrap_or("");
    assert_eq!(status, "error");
    let trace = doc
        .get("trace")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if let Some(policy_status) = trace
        .iter()
        .find(|entry| {
            entry.get("component").and_then(|c| c.as_str()) == Some("component.tool.external")
        })
        .and_then(|entry| entry.get("payload"))
        .and_then(|p| p.get("policy_status"))
        .and_then(|v| v.as_str())
    {
        assert_eq!(policy_status, "blocked_by_policy");
    }
    Ok(())
}

#[test]
fn pack_realism_l4_1_cli_external_mocked_and_secret_loaded() -> Result<()> {
    let workspace = Workspace::new("realism-l4.1-mocked")?;
    let pack_bytes = build_l4_pack()?;
    let pack_path = workspace.root.join("l4.gtpack");
    std::fs::write(&pack_path, &pack_bytes)?;

    // Provide secret via seed file; mock external allowed.
    let seed_path = workspace.root.join("secrets.yaml");
    std::fs::write(&seed_path, "entries:\n- uri: API_KEY\n  text: abc123\n")?;
    let (code, stdout, stderr) = run_cli(
        &pack_path,
        r#"{"query":"hi"}"#,
        false,
        true,
        true,
        Some(&seed_path),
    )?;
    if code != 0 {
        diag_with_owner(
            "pack_realism_l4_1_cli_external_mocked_and_secret_loaded",
            "execute",
            &workspace,
            &format!("exit {code}, stderr: {stderr}"),
            "greentic-dev",
        );
        anyhow::bail!("cli exit code {code}");
    }
    let doc = parse_json(&stdout)?;
    let trace = doc
        .get("trace")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let policy_status = trace
        .iter()
        .find(|entry| {
            entry.get("component").and_then(|c| c.as_str()) == Some("component.tool.external")
        })
        .and_then(|entry| entry.get("payload"))
        .and_then(|p| p.get("policy_status"))
        .and_then(|v| v.as_str());
    assert_eq!(policy_status, Some("mocked_external"));
    let secret_prefix = doc
        .get("trace")
        .and_then(|t| t.as_array())
        .and_then(|arr| {
            arr.iter().find(|entry| {
                entry.get("component").and_then(|c| c.as_str()) == Some("component.tool.secret")
            })
        })
        .and_then(|entry| entry.get("payload"))
        .and_then(|p| p.get("prefix"))
        .and_then(|p| p.as_str())
        .unwrap_or("");
    assert_eq!(secret_prefix, "abc");
    assert!(
        !stdout.contains("abc123") && !stderr.contains("abc123"),
        "secret value must not leak"
    );
    assert!(
        !trace.is_empty(),
        "expected trace entries in CLI json output"
    );
    Ok(())
}
