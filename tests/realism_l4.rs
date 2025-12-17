mod support;

use std::sync::Arc;

use anyhow::Result;
use serde_json::json;
use support::exec::{ExecOptions, MemorySecrets, execute_flow_from_pack_with_options};
use support::l4::build_l4_pack;
use support::{Workspace, diag_with_owner};

fn default_options() -> ExecOptions {
    ExecOptions {
        mock_external: true,
        ..ExecOptions::default()
    }
}

#[test]
fn pack_realism_l4_missing_secret_fails_cleanly() -> Result<()> {
    let _ws = Workspace::new("realism-l4-missing")?;
    let pack = build_l4_pack()?;
    let opts = default_options();

    let result =
        execute_flow_from_pack_with_options(&pack, "default", json!({ "query": "hi" }), &opts)?;
    assert_eq!(result.status, "error");
    let secret = result
        .trace
        .iter()
        .find(|t| t.component == "component.tool.secret")
        .expect("secret node trace");
    assert_eq!(secret.status, "error");
    assert!(
        secret
            .payload
            .get("secret_lookup")
            .and_then(|v| v.get("status"))
            .and_then(|v| v.as_str())
            == Some("missing")
    );
    Ok(())
}

#[test]
fn pack_realism_l4_secret_injected_succeeds() -> Result<()> {
    let _ws = Workspace::new("realism-l4-secret")?;
    let pack = build_l4_pack()?;
    let mem = MemorySecrets::new();
    mem.insert_str("API_KEY", "abc123");
    let opts = ExecOptions {
        secrets: Arc::new(mem.clone()),
        mock_external: true,
        ..ExecOptions::default()
    };

    let result =
        execute_flow_from_pack_with_options(&pack, "default", json!({ "query": "hi" }), &opts)?;
    assert_eq!(result.status, "ok");
    let out = result.output.to_string();
    assert!(
        !out.contains("abc123"),
        "secret value should not leak in output"
    );
    let secret = result
        .trace
        .iter()
        .find(|t| t.component == "component.tool.secret")
        .expect("secret node trace");
    assert_eq!(secret.status, "ok");
    assert!(secret.payload.get("prefix").and_then(|v| v.as_str()) == Some("abc"));
    Ok(())
}

#[test]
fn pack_realism_l4_external_call_blocked_by_policy() -> Result<()> {
    let ws = Workspace::new("realism-l4-blocked")?;
    let pack = build_l4_pack()?;
    let mem = MemorySecrets::new();
    mem.insert_str("API_KEY", "abc123");
    let opts = ExecOptions {
        offline: true,
        external_enabled: false,
        mock_external: false,
        secrets: Arc::new(mem),
        ..ExecOptions::default()
    };

    let result =
        execute_flow_from_pack_with_options(&pack, "default", json!({ "query": "hi" }), &opts)?;
    if result.status != "error" {
        diag_with_owner(
            "pack_realism_l4_external_call_blocked_by_policy",
            "execute",
            &ws,
            "expected policy error",
            "greentic-dev",
        );
    }
    assert_eq!(result.status, "error");
    let external = result
        .trace
        .iter()
        .find(|t| t.component == "component.tool.external")
        .expect("external trace");
    assert_eq!(external.status, "error");
    let policy = external.payload.get("policy").cloned().unwrap_or_default();
    assert_eq!(policy.get("offline").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(
        policy.get("external_enabled").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert!(
        external
            .payload
            .get("policy_status")
            .and_then(|v| v.as_str())
            == Some("blocked_by_policy")
    );
    Ok(())
}

#[test]
fn pack_realism_l4_external_call_allowed_but_mocked() -> Result<()> {
    let _ws = Workspace::new("realism-l4-mocked")?;
    let pack = build_l4_pack()?;
    let mem = MemorySecrets::new();
    mem.insert_str("API_KEY", "abc123");
    let opts = ExecOptions {
        offline: false,
        external_enabled: true,
        mock_external: true,
        secrets: Arc::new(mem),
        mock_external_payload: json!({"mocked": true, "source": "test-fixture", "data": {"n": 7}}),
    };

    let result =
        execute_flow_from_pack_with_options(&pack, "default", json!({ "query": "hi" }), &opts)?;
    assert_eq!(result.status, "ok");
    let external = result
        .trace
        .iter()
        .find(|t| t.component == "component.tool.external")
        .expect("external trace");
    assert_eq!(
        external
            .payload
            .get("policy_status")
            .and_then(|v| v.as_str()),
        Some("mocked_external")
    );
    assert_eq!(
        external
            .payload
            .get("result")
            .and_then(|v| v.get("data"))
            .and_then(|v| v.get("n"))
            .and_then(|v| v.as_i64()),
        Some(7)
    );
    Ok(())
}
