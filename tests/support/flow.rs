#![allow(dead_code)]
#![allow(clippy::collapsible_if)]

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value as JsonValue;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSummary {
    pub keys: HashSet<String>,
    pub has_out_map: bool,
    pub has_err_map: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedFlow {
    pub nodes: HashSet<String>,
    pub edges: HashSet<(String, String)>,
    pub node_summaries: HashMap<String, NodeSummary>,
}

pub fn load_default_flow(manifest_path: &Path) -> Result<JsonValue> {
    let raw = fs::read_to_string(manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let manifest: JsonValue = serde_json::from_str(&raw)
        .with_context(|| format!("invalid JSON in {}", manifest_path.display()))?;

    let dev_flows = manifest
        .get("dev_flows")
        .and_then(|v| v.as_object())
        .ok_or(missing_default_err())?;
    let Some(default) = dev_flows.get("default") else {
        return Err(missing_default_err());
    };
    default
        .get("graph")
        .cloned()
        .ok_or_else(|| anyhow!("dev_flows.default missing graph"))
}

pub fn validate_graph(graph: &JsonValue) -> Result<NormalizedFlow> {
    let obj = graph
        .as_object()
        .ok_or_else(|| anyhow!("graph must be an object"))?;
    let nodes = obj
        .get("nodes")
        .and_then(|v| v.as_object())
        .ok_or_else(|| anyhow!("graph missing nodes object"))?;

    let mut node_ids = HashSet::new();
    let mut summaries = HashMap::new();
    for (id, node) in nodes {
        if !node_ids.insert(id.clone()) {
            bail!("duplicate node id `{id}`");
        }
        let node_obj = node
            .as_object()
            .ok_or_else(|| anyhow!("node `{id}` is not an object"))?;
        let keys = node_obj.keys().cloned().collect();
        let has_out_map = node_obj.get("out_map").is_some();
        let has_err_map = node_obj.get("err_map").is_some();
        summaries.insert(
            id.clone(),
            NodeSummary {
                keys,
                has_out_map,
                has_err_map,
            },
        );
    }

    let mut edges = HashSet::new();
    if let Some(edge_entries) = obj.get("edges").and_then(|v| v.as_array()) {
        for edge in edge_entries {
            let edge_obj = edge
                .as_object()
                .ok_or_else(|| anyhow!("edge entry is not an object"))?;
            let from = edge_obj
                .get("from")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("edge missing from"))?;
            let to = edge_obj
                .get("to")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("edge missing to"))?;
            edges.insert((from.to_string(), to.to_string()));
        }
    }

    for (id, node) in nodes {
        if let Some(routes) = node
            .as_object()
            .and_then(|n| n.get("routing"))
            .and_then(|r| r.as_array())
        {
            for entry in routes {
                let Some(route_obj) = entry.as_object() else {
                    bail!("node `{id}` has non-object routing entry");
                };
                if let Some(to) = route_obj.get("to").and_then(|v| v.as_str()) {
                    edges.insert((id.clone(), to.to_string()));
                }
            }
        }
    }

    let start = obj.get("start").and_then(|v| v.as_str());
    if let Some(start) = start {
        if !node_ids.contains(start) {
            bail!("graph start `{start}` not found in nodes");
        }
    }

    for (from, to) in &edges {
        if !node_ids.contains(from) {
            bail!("edge references unknown `from` node `{from}`");
        }
        if !node_ids.contains(to) {
            bail!("edge references unknown `to` node `{to}`");
        }
    }

    Ok(NormalizedFlow {
        nodes: node_ids,
        edges,
        node_summaries: summaries,
    })
}

pub fn roundtrip(graph: &JsonValue) -> Result<(JsonValue, NormalizedFlow)> {
    let serialized = serde_json::to_string(graph).context("serialize graph")?;
    let reparsed: JsonValue = serde_json::from_str(&serialized).context("reparse graph")?;
    let normalized = validate_graph(&reparsed)?;
    Ok((reparsed, normalized))
}

pub fn assert_semantic_eq(lhs: &NormalizedFlow, rhs: &NormalizedFlow) -> Result<()> {
    if lhs.nodes != rhs.nodes {
        bail!("node sets differ: lhs={:?}, rhs={:?}", lhs.nodes, rhs.nodes);
    }
    if lhs.edges != rhs.edges {
        bail!("edge sets differ: lhs={:?}, rhs={:?}", lhs.edges, rhs.edges);
    }
    if lhs.node_summaries.len() != rhs.node_summaries.len() {
        bail!("node summaries length differ");
    }
    for (id, left_summary) in &lhs.node_summaries {
        let Some(right_summary) = rhs.node_summaries.get(id) else {
            bail!("node summary missing for `{id}`");
        };
        if left_summary.keys != right_summary.keys
            || left_summary.has_out_map != right_summary.has_out_map
            || left_summary.has_err_map != right_summary.has_err_map
        {
            bail!("node summary mismatch for `{id}`");
        }
    }
    Ok(())
}

pub fn missing_default_err() -> anyhow::Error {
    anyhow!(
        "Flow 'default' is missing from manifest.dev_flows. Run `greentic-component flow update` to regenerate config flows."
    )
}

pub fn add_node_after(
    manifest_path: &Path,
    new_id: &str,
    after: &str,
) -> Result<(JsonValue, JsonValue)> {
    let raw = fs::read_to_string(manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let mut manifest: JsonValue = serde_json::from_str(&raw)
        .with_context(|| format!("invalid JSON in {}", manifest_path.display()))?;
    let mut graph = load_default_flow(manifest_path)?;

    let graph_obj = graph
        .as_object_mut()
        .ok_or_else(|| anyhow!("graph must be an object"))?;
    let nodes = graph_obj
        .get_mut("nodes")
        .and_then(|v| v.as_object_mut())
        .ok_or_else(|| anyhow!("graph missing nodes object"))?;
    if !nodes.contains_key(after) {
        bail!("node `{after}` not found");
    }
    if nodes.contains_key(new_id) {
        bail!("node `{new_id}` already exists");
    }

    let mut new_node = serde_json::Map::new();
    if nodes.contains_key("end") {
        new_node.insert(
            "routing".to_string(),
            JsonValue::Array(vec![JsonValue::Object(
                [("to".to_string(), JsonValue::String("end".into()))]
                    .into_iter()
                    .collect(),
            )]),
        );
    }
    nodes.insert(new_id.to_string(), JsonValue::Object(new_node));

    // update routing on the `after` node
    if let Some(map) = nodes.get_mut(after).and_then(|v| v.as_object_mut()) {
        let routing = map
            .entry("routing")
            .or_insert_with(|| JsonValue::Array(Vec::new()));
        let seq = routing
            .as_array_mut()
            .ok_or_else(|| anyhow!("routing for `{after}` is not an array"))?;
        seq.push(JsonValue::Object(
            [("to".to_string(), JsonValue::String(new_id.to_string()))]
                .into_iter()
                .collect(),
        ));
    }

    let edges = graph_obj
        .entry("edges")
        .or_insert_with(|| JsonValue::Array(Vec::new()));
    if let Some(edges_arr) = edges.as_array_mut() {
        edges_arr.push(JsonValue::Object(
            [
                ("from".to_string(), JsonValue::String(after.to_string())),
                ("to".to_string(), JsonValue::String(new_id.to_string())),
            ]
            .into_iter()
            .collect(),
        ));
    }

    // persist into manifest
    if let Some(obj) = manifest.as_object_mut() {
        if let Some(flows) = obj.get_mut("dev_flows").and_then(|v| v.as_object_mut()) {
            if let Some(default) = flows.get_mut("default").and_then(|v| v.as_object_mut()) {
                default.insert("graph".to_string(), graph.clone());
            }
        }
    }

    let before: JsonValue = serde_json::from_str(&raw)?;
    fs::write(manifest_path, serde_json::to_string_pretty(&manifest)?)
        .with_context(|| format!("failed to write {}", manifest_path.display()))?;
    Ok((before, manifest))
}

pub fn replace_default_flow(manifest_path: &Path, graph: JsonValue) -> Result<()> {
    let raw = fs::read_to_string(manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let mut manifest: JsonValue = serde_json::from_str(&raw)
        .with_context(|| format!("invalid JSON in {}", manifest_path.display()))?;
    let flows = manifest
        .as_object_mut()
        .ok_or_else(|| anyhow!("manifest must be object"))?
        .entry("dev_flows")
        .or_insert_with(|| JsonValue::Object(Default::default()));
    let map = flows
        .as_object_mut()
        .ok_or_else(|| anyhow!("dev_flows must be object"))?;
    map.insert(
        "default".to_string(),
        JsonValue::Object(
            [
                (
                    "format".to_string(),
                    JsonValue::String("flow-ir-json".into()),
                ),
                ("graph".to_string(), graph),
            ]
            .into_iter()
            .collect(),
        ),
    );
    fs::write(manifest_path, serde_json::to_string_pretty(&manifest)?)
        .with_context(|| format!("failed to write {}", manifest_path.display()))?;
    Ok(())
}
