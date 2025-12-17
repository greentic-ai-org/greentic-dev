#![allow(clippy::collapsible_if)]

mod support;

use anyhow::{Context, Result};
use greentic_dev::pack_verify::{self, VerifyPolicy};
use serde_json::Value as JsonValue;
use support::flow::{
    add_node_after, assert_semantic_eq, load_default_flow, replace_default_flow, roundtrip,
    validate_graph,
};
use support::{
    Workspace, build_pack, copy_fixture_component, diag_with_owner, load_gtpack, write_pack_flow,
};

fn load_fixture_flow() -> Result<JsonValue> {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let flow_path = root.join("tests/fixtures/l2/default_dev_flow.json");
    let data = std::fs::read_to_string(&flow_path)
        .with_context(|| format!("failed to read {}", flow_path.display()))?;
    serde_json::from_str(&data).context("invalid fixture flow JSON")
}

#[test]
fn pack_realism_l2_flow_validates() -> Result<()> {
    let workspace = Workspace::new("realism-l2-validate")?;
    let component_dir = copy_fixture_component(&workspace, false)?;
    replace_default_flow(
        &component_dir.join("component.manifest.json"),
        load_fixture_flow()?,
    )?;
    let flow_path = write_pack_flow(&workspace, "hello-flow")?;

    let pack_path = build_pack(
        &workspace,
        &flow_path,
        component_dir.parent().expect("component root"),
    )?;
    let (manifest, _) = load_gtpack(&pack_path)?;
    let manifest_path = component_dir.join("component.manifest.json");
    let graph = load_default_flow(&manifest_path)?;
    validate_graph(&graph).map_err(|err| {
        diag_with_owner(
            "pack_realism_l2_flow_validates",
            "validate",
            &workspace,
            &format!("failed: {err}"),
            "greentic-flow",
        );
        err
    })?;
    pack_verify::run(&pack_path, VerifyPolicy::DevOk, false)
        .context("pack verify should succeed")?;

    // also ensure the pack contains the component manifest with dev_flows
    assert!(
        manifest
            .components
            .iter()
            .any(|c| c.name == "dev.greentic.echo"),
        "pack must include dev.greentic.echo"
    );
    Ok(())
}

#[test]
fn pack_realism_l2_flow_roundtrip_stable() -> Result<()> {
    let workspace = Workspace::new("realism-l2-roundtrip")?;
    let component_dir = copy_fixture_component(&workspace, false)?;
    replace_default_flow(
        &component_dir.join("component.manifest.json"),
        load_fixture_flow()?,
    )?;

    let graph = load_default_flow(&component_dir.join("component.manifest.json"))?;
    let normalized = validate_graph(&graph)?;
    let (reparsed, normalized_rt) = roundtrip(&graph)?;
    assert_semantic_eq(&normalized, &normalized_rt).map_err(|err| {
        diag_with_owner(
            "pack_realism_l2_flow_roundtrip_stable",
            "roundtrip",
            &workspace,
            &format!("semantic mismatch: {err}"),
            "greentic-flow",
        );
        err
    })?;
    // sanity: roundtripped graph should still validate
    validate_graph(&reparsed)?;
    Ok(())
}

#[test]
fn pack_realism_l2_invalid_graph_fails_cleanly() -> Result<()> {
    let workspace = Workspace::new("realism-l2-invalid")?;
    let component_dir = copy_fixture_component(&workspace, false)?;
    replace_default_flow(
        &component_dir.join("component.manifest.json"),
        load_fixture_flow()?,
    )?;

    let manifest_path = component_dir.join("component.manifest.json");
    let mut graph = load_default_flow(&manifest_path)?;
    // remove node referenced by edge
    if let Some(nodes) = graph
        .as_object_mut()
        .and_then(|o| o.get_mut("nodes"))
        .and_then(|n| n.as_object_mut())
    {
        nodes.remove("step_c");
    }
    let err = validate_graph(&graph).expect_err("validation should fail");
    diag_with_owner(
        "pack_realism_l2_invalid_graph_fails_cleanly",
        "validate",
        &workspace,
        &format!("expected failure: {err}"),
        "greentic-flow",
    );
    assert!(
        err.to_string().contains("unknown `to` node") || err.to_string().contains("unknown `from`"),
        "error should mention missing node"
    );
    Ok(())
}

#[test]
fn pack_realism_l2_flow_edit_mutates_manifest_correctly() -> Result<()> {
    let workspace = Workspace::new("realism-l2-edit")?;
    let component_dir = copy_fixture_component(&workspace, false)?;
    replace_default_flow(
        &component_dir.join("component.manifest.json"),
        load_fixture_flow()?,
    )?;

    let manifest_path = component_dir.join("component.manifest.json");
    let (before, after) = add_node_after(&manifest_path, "new_node", "start")?;
    assert!(before != after, "manifest JSON should change after edit");

    // ensure only dev_flows.default.graph changed
    let mut stripped_before = before.clone();
    let mut stripped_after = after.clone();
    if let Some(obj) = stripped_before.as_object_mut() {
        if let Some(flows) = obj.get_mut("dev_flows").and_then(|v| v.as_object_mut()) {
            if let Some(default) = flows.get_mut("default").and_then(|v| v.as_object_mut()) {
                default.remove("graph");
            }
        }
    }
    if let Some(obj) = stripped_after.as_object_mut() {
        if let Some(flows) = obj.get_mut("dev_flows").and_then(|v| v.as_object_mut()) {
            if let Some(default) = flows.get_mut("default").and_then(|v| v.as_object_mut()) {
                default.remove("graph");
            }
        }
    }
    assert_eq!(
        stripped_before, stripped_after,
        "non-graph fields should remain unchanged"
    );

    // graph still validates after edit
    let graph_after = load_default_flow(&manifest_path)?;
    let normalized_after = validate_graph(&graph_after)?;
    let normalized_before = validate_graph(&load_fixture_flow()?)?;
    assert!(
        normalized_after.nodes.len() == normalized_before.nodes.len() + 1,
        "node count should increase by 1"
    );
    Ok(())
}
