mod support;

use std::collections::HashMap;

use anyhow::{Context, Result};
use serde_json::json;
use support::custom_add_step::custom_add_step;
use support::flow::{
    assert_semantic_eq, load_default_flow, replace_default_flow, roundtrip, validate_graph,
};
use support::{
    Workspace, build_pack, copy_fixture_component_with_schema, diag_with_owner, write_pack_flow,
};

fn l2_fixture_flow() -> Result<serde_json::Value> {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let flow_path = root.join("tests/fixtures/l2/default_dev_flow.json");
    let data = std::fs::read_to_string(&flow_path)
        .with_context(|| format!("failed to read {}", flow_path.display()))?;
    serde_json::from_str(&data).context("invalid fixture flow JSON")
}

fn schema_with_optional_prompt() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "required_field": { "type": "string" },
            "base_url": { "type": "string", "x-flow-ask": true, "x-flow-prompt": "Base URL" },
            "timeout_ms": { "type": "number" }
        },
        "required": ["required_field"]
    })
}

#[test]
fn pack_realism_l2_1_custom_add_step_prompts_for_optional_fields() -> Result<()> {
    let workspace = Workspace::new("realism-l2.1-custom")?;
    let component_dir =
        copy_fixture_component_with_schema(&workspace, false, Some(schema_with_optional_prompt()))?;
    replace_default_flow(
        &component_dir.join("component.manifest.json"),
        l2_fixture_flow()?,
    )?;
    let flow_path = write_pack_flow(&workspace, "hello-flow")?;
    build_pack(
        &workspace,
        &flow_path,
        component_dir.parent().expect("component root"),
    )?;

    let mut answers = HashMap::new();
    answers.insert("required_field".to_string(), json!("req"));
    answers.insert("base_url".to_string(), json!("https://api.example.com"));

    custom_add_step(
        &component_dir.join("component.manifest.json"),
        "dev.greentic.echo",
        &answers,
    )?;

    let graph = load_default_flow(&component_dir.join("component.manifest.json"))?;
    let normalized = validate_graph(&graph)?;
    let (_rt_graph, rt_norm) = roundtrip(&graph)?;
    assert_semantic_eq(&normalized, &rt_norm).map_err(|err| {
        diag_with_owner(
            "pack_realism_l2_1_custom_add_step_prompts_for_optional_fields",
            "custom add-step schema prompt",
            &workspace,
            &format!("roundtrip mismatch: {err}"),
            "greentic-dev",
        );
        err
    })?;

    let custom_node = graph
        .get("nodes")
        .and_then(|n| n.get("custom_step"))
        .and_then(|v| v.as_object())
        .ok_or_else(|| anyhow::anyhow!("custom_step missing"))?;
    let config = custom_node
        .get("component.exec")
        .and_then(|v| v.as_object())
        .ok_or_else(|| anyhow::anyhow!("component.exec missing"))?;
    let input = config
        .get("input")
        .and_then(|v| v.as_object())
        .ok_or_else(|| anyhow::anyhow!("input missing"))?;
    assert!(
        input.contains_key("base_url"),
        "base_url should be included from prompt answers"
    );

    Ok(())
}

#[test]
fn pack_realism_l2_1_default_add_step_does_not_require_optional_fields() -> Result<()> {
    let workspace = Workspace::new("realism-l2.1-default")?;
    let component_dir =
        copy_fixture_component_with_schema(&workspace, false, Some(schema_with_optional_prompt()))?;
    replace_default_flow(
        &component_dir.join("component.manifest.json"),
        l2_fixture_flow()?,
    )?;

    // simulate default add-step by adding a simple node without prompts
    support::flow::add_node_after(
        &component_dir.join("component.manifest.json"),
        "plain",
        "start",
    )?;

    let graph = load_default_flow(&component_dir.join("component.manifest.json"))?;
    validate_graph(&graph)?;
    let node = graph
        .get("nodes")
        .and_then(|n| n.get("plain"))
        .and_then(|v| v.as_object())
        .ok_or_else(|| anyhow::anyhow!("plain node missing"))?;
    // default path should not inject optional ask fields
    if let Some(exec) = node.get("component.exec").and_then(|v| v.as_object())
        && let Some(input) = exec.get("input").and_then(|v| v.as_object())
    {
        assert!(
            !input.contains_key("base_url"),
            "default add-step should not include prompt-only optional fields"
        );
    }
    Ok(())
}

#[test]
fn pack_realism_l2_1_custom_add_step_rejects_invalid_input_cleanly() -> Result<()> {
    let workspace = Workspace::new("realism-l2.1-invalid")?;
    let component_dir =
        copy_fixture_component_with_schema(&workspace, false, Some(schema_with_optional_prompt()))?;
    replace_default_flow(
        &component_dir.join("component.manifest.json"),
        l2_fixture_flow()?,
    )?;

    let manifest_path = component_dir.join("component.manifest.json");
    let before = load_default_flow(&manifest_path)?;

    let mut answers = HashMap::new();
    answers.insert("required_field".to_string(), json!("req"));
    answers.insert("base_url".to_string(), json!(123)); // invalid type

    let err = custom_add_step(&manifest_path, "dev.greentic.echo", &answers)
        .expect_err("expected type validation failure");
    assert!(
        err.to_string().contains("type check"),
        "should mention type check failure"
    );

    let after = load_default_flow(&manifest_path)?;
    assert_eq!(
        before, after,
        "graph should remain unchanged on validation failure"
    );
    Ok(())
}
