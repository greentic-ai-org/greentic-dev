use std::fs;
use std::path::Path;

use crate::cli::{ConfigFlowModeArg, FlowAddStepArgs};
use crate::component_add::run_component_add;
use crate::pack_init::PackInitIntent;
use crate::path_safety::normalize_under_root;
use anyhow::{Context, Result, anyhow, bail};
use greentic_flow::flow_bundle::load_and_validate_bundle;
use serde_json::Value as JsonValue;
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;
use std::{io, io::IsTerminal};

use greentic_types::component::ComponentManifest;

pub fn validate(path: &Path, compact_json: bool) -> Result<()> {
    let root = std::env::current_dir()
        .context("failed to resolve workspace root")?
        .canonicalize()
        .context("failed to canonicalize workspace root")?;
    let safe = normalize_under_root(&root, path)?;
    let source = fs::read_to_string(&safe)
        .with_context(|| format!("failed to read flow definition at {}", safe.display()))?;

    let bundle = load_and_validate_bundle(&source, Some(&safe)).with_context(|| {
        format!(
            "flow validation failed for {} using greentic-flow",
            safe.display()
        )
    })?;

    let serialized = if compact_json {
        serde_json::to_string(&bundle)?
    } else {
        serde_json::to_string_pretty(&bundle)?
    };

    println!("{serialized}");
    Ok(())
}

pub fn run_add_step(args: FlowAddStepArgs) -> Result<()> {
    let manifest_path = args
        .manifest
        .clone()
        .unwrap_or_else(|| PathBuf::from("component.manifest.json"));
    if !manifest_path.exists() {
        bail!(
            "component.manifest.json not found at {}. Use --manifest to point to the manifest file.",
            manifest_path.display()
        );
    }
    let manifest_raw = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let manifest: ComponentManifest = serde_json::from_str(&manifest_raw).with_context(|| {
        format!(
            "failed to parse component manifest JSON at {}",
            manifest_path.display()
        )
    })?;
    let mut manifest_json: JsonValue = serde_json::from_str(&manifest_raw).with_context(|| {
        format!(
            "failed to parse manifest JSON at {}",
            manifest_path.display()
        )
    })?;
    let flow_id = if args.flow == "default" {
        args.flow_id.clone()
    } else {
        args.flow.clone()
    };
    let flow_key = flow_id.parse().map_err(|_| {
        anyhow!(
            "invalid flow identifier `{}`; flow ids must be valid FlowId strings",
            flow_id
        )
    })?;
    let Some(snapshot_graph) = manifest
        .dev_flows
        .get(&flow_key)
        .and_then(|flow| flow.graph.as_object().cloned())
    else {
        bail!(
            "Flow `{}` is missing from manifest.dev_flows. Run `greentic-component flow update` to regenerate config flows.",
            flow_id
        );
    };

    let flow_entry = manifest_json
        .get_mut("dev_flows")
        .and_then(|flows| flows.as_object_mut())
        .and_then(|flows| flows.get_mut(&flow_id))
        .ok_or_else(|| anyhow!("flow `{}` is missing from manifest.dev_flows", flow_id))?;
    let graph_value = flow_entry
        .as_object_mut()
        .and_then(|obj| obj.get_mut("graph"))
        .ok_or_else(|| anyhow!("flow `{}` missing graph object", flow_id))?;
    let graph_obj = graph_value
        .as_object_mut()
        .ok_or_else(|| anyhow!("flow `{}` graph is not an object", flow_id))?;

    let coord = args
        .coordinate
        .ok_or_else(|| anyhow!("component coordinate is required (pass --coordinate)"))?;

    // Fetch or use local component bundle
    let bundle_dir = resolve_component_bundle(&coord, args.profile.as_deref())?;
    let flows_dir = bundle_dir.join("flows");
    let custom_flow = flows_dir.join("custom.ygtc");
    let default_flow = flows_dir.join("default.ygtc");
    let selected = match args.mode {
        Some(ConfigFlowModeArg::Custom) => {
            if custom_flow.exists() {
                custom_flow
            } else {
                default_flow.clone()
            }
        }
        Some(ConfigFlowModeArg::Default) => {
            if default_flow.exists() {
                default_flow
            } else {
                custom_flow.clone()
            }
        }
        None => {
            if default_flow.exists() {
                default_flow
            } else if custom_flow.exists() {
                custom_flow
            } else {
                bail!("component bundle does not provide flows/default.ygtc or flows/custom.ygtc")
            }
        }
    };
    if !selected.exists() {
        bail!("selected config flow missing at {}", selected.display());
    }

    let output = crate::pack_run::run_config_flow(&selected)
        .with_context(|| format!("failed to run config flow {}", selected.display()))?;
    let (node_id, mut node) = parse_config_flow_output(&output)?;
    let graph_snapshot = JsonValue::Object(snapshot_graph.clone());
    let after = args
        .after
        .clone()
        .or_else(|| prompt_routing_target(&graph_snapshot));
    if let Some(after) = after.as_deref() {
        patch_placeholder_routing(&mut node, after);
    }

    // Update target flow JSON
    let nodes = graph_obj
        .get_mut("nodes")
        .and_then(|n| n.as_object_mut())
        .ok_or_else(|| anyhow!("flow `{}` missing nodes map", flow_id))?;
    nodes.insert(node_id.clone(), node);

    if let Some(after) = args.after {
        append_routing(graph_obj, &after, &node_id)?;
    } else if let Some(after) = after.as_deref() {
        append_routing(graph_obj, after, &node_id)?;
    }

    let rendered = serde_json::to_string_pretty(&manifest_json)
        .context("failed to render updated manifest")?;
    std::fs::write(&manifest_path, rendered)
        .with_context(|| format!("failed to write {}", manifest_path.display()))?;

    println!(
        "Added node `{}` from config flow {} to {}",
        node_id,
        selected.file_name().unwrap_or_default().to_string_lossy(),
        manifest_path.display()
    );
    Ok(())
}

fn prompt_routing_target(flow_doc: &JsonValue) -> Option<String> {
    if !io::stdout().is_terminal() {
        return None;
    }
    let nodes = flow_doc
        .as_object()
        .and_then(|m| m.get("nodes"))
        .and_then(|n| n.as_object())?;
    let mut keys: Vec<String> = nodes.keys().cloned().collect();
    keys.sort();
    if keys.is_empty() {
        return None;
    }

    println!("Select where to route from (empty to skip):");
    for (idx, key) in keys.iter().enumerate() {
        println!("  {}) {}", idx + 1, key);
    }
    print!("Choice: ");
    let _ = io::stdout().flush();
    let mut buf = String::new();
    if io::stdin().read_line(&mut buf).is_err() {
        return None;
    }
    let choice = buf.trim();
    if choice.is_empty() {
        return None;
    }
    if let Ok(idx) = choice.parse::<usize>()
        && idx >= 1
        && idx <= keys.len()
    {
        return Some(keys[idx - 1].clone());
    }
    None
}

fn patch_placeholder_routing(node: &mut JsonValue, next: &str) {
    let Some(map) = node.as_object_mut() else {
        return;
    };
    let Some(routing) = map.get_mut("routing") else {
        return;
    };
    let Some(routes) = routing.as_array_mut() else {
        return;
    };
    for entry in routes.iter_mut() {
        if let Some(JsonValue::String(to)) =
            entry.as_object_mut().and_then(|route| route.get_mut("to"))
            && to == "NEXT_NODE_PLACEHOLDER"
        {
            *to = next.to_string();
        }
    }
}

fn resolve_component_bundle(coordinate: &str, profile: Option<&str>) -> Result<PathBuf> {
    let path = PathBuf::from_str(coordinate).unwrap_or_default();
    if path.exists() {
        return Ok(path);
    }
    let dir = run_component_add(coordinate, profile, PackInitIntent::Dev)?;
    Ok(dir)
}

pub fn parse_config_flow_output(output: &str) -> Result<(String, JsonValue)> {
    let value: JsonValue =
        serde_json::from_str(output).context("config flow output is not valid JSON")?;
    let obj = value
        .as_object()
        .ok_or_else(|| anyhow!("config flow output must be a JSON object"))?;
    let node_id = obj
        .get("node_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("config flow output missing node_id"))?
        .to_string();
    let node = obj
        .get("node")
        .ok_or_else(|| anyhow!("config flow output missing node"))?
        .clone();
    if !node.is_object() {
        bail!("config flow output node must be an object");
    }
    Ok((node_id, node))
}

fn append_routing(
    flow_doc: &mut serde_json::Map<String, JsonValue>,
    from_node: &str,
    to_node: &str,
) -> Result<()> {
    let nodes = flow_doc
        .get_mut("nodes")
        .and_then(|n| n.as_object_mut())
        .ok_or_else(|| anyhow!("flow missing nodes map"))?;
    let Some(node) = nodes.get_mut(from_node) else {
        bail!("node `{from_node}` not found for routing update");
    };
    let mapping = node
        .as_object_mut()
        .ok_or_else(|| anyhow!("node `{from_node}` is not an object"))?;
    let routing = mapping
        .entry("routing")
        .or_insert(JsonValue::Array(Vec::new()));
    let seq = routing
        .as_array_mut()
        .ok_or_else(|| anyhow!("routing for `{from_node}` is not a list"))?;
    let mut entry = serde_json::Map::new();
    entry.insert("to".to_string(), JsonValue::String(to_node.to_string()));
    seq.push(JsonValue::Object(entry));
    Ok(())
}
