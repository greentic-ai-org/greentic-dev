use std::fs;
use std::path::Path;

use crate::cli::{ConfigFlowModeArg, FlowAddStepArgs};
use crate::pack_init::{self, PackInitIntent};
use crate::path_safety::normalize_under_root;
use anyhow::{Context, Result, anyhow, bail};
use greentic_flow::flow_bundle::load_and_validate_bundle;
use serde_json::Value as JsonValue;
use serde_yaml_bw as serde_yaml;
use serde_yaml_bw::Mapping as YamlMapping;
use serde_yaml_bw::Sequence as YamlSequence;
use serde_yaml_bw::Value as YamlValue;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;
use std::{io, io::IsTerminal};

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
    let root = std::env::current_dir()
        .context("failed to resolve workspace root")?
        .canonicalize()
        .context("failed to canonicalize workspace root")?;
    let flow_path = root.join("flows").join(format!("{}.ygtc", args.flow_id));
    if !flow_path.exists() {
        bail!("flow file not found: {}", flow_path.display());
    }
    let flow_src = std::fs::read_to_string(&flow_path)
        .with_context(|| format!("failed to read {}", flow_path.display()))?;
    let mut flow_doc: YamlValue = serde_yaml::from_str(&flow_src)
        .with_context(|| format!("failed to parse {}", flow_path.display()))?;

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
    let after = args
        .after
        .clone()
        .or_else(|| prompt_routing_target(&flow_doc));
    if let Some(after) = after.as_deref() {
        patch_placeholder_routing(&mut node, after);
    }

    // Update target flow YAML
    let nodes = flow_doc
        .as_mapping_mut()
        .and_then(|m| m.get_mut(YamlValue::String("nodes".to_string(), None)))
        .and_then(|n| n.as_mapping_mut())
        .ok_or_else(|| anyhow!("flow missing nodes map"))?;
    nodes.insert(
        YamlValue::String(node_id.clone(), None),
        node_to_yaml(node)?,
    );

    if let Some(after) = args.after {
        append_routing(&mut flow_doc, &after, &node_id)?;
    } else if let Some(after) = after.as_deref() {
        append_routing(&mut flow_doc, after, &node_id)?;
    }

    let rendered = serde_yaml::to_string(&flow_doc).context("failed to render updated flow")?;
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&flow_path)
        .with_context(|| format!("failed to open {} for writing", flow_path.display()))?;
    file.write_all(rendered.as_bytes())
        .with_context(|| format!("failed to write {}", flow_path.display()))?;

    println!(
        "Added node `{}` from config flow {} to {}",
        node_id,
        selected.file_name().unwrap_or_default().to_string_lossy(),
        flow_path.display()
    );
    Ok(())
}

fn prompt_routing_target(flow_doc: &YamlValue) -> Option<String> {
    if !io::stdout().is_terminal() {
        return None;
    }
    let nodes = flow_doc
        .as_mapping()
        .and_then(|m| m.get(YamlValue::String("nodes".to_string(), None)))
        .and_then(|n| n.as_mapping())?;
    let mut keys: Vec<String> = nodes
        .keys()
        .filter_map(|k| k.as_str().map(|s| s.to_string()))
        .collect();
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
    let dir = pack_init::run_component_add(coordinate, profile, PackInitIntent::Dev)?;
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

fn node_to_yaml(node: JsonValue) -> Result<YamlValue> {
    let yaml_string = serde_yaml::to_string(&node).context("failed to render node to YAML")?;
    let yaml_value: YamlValue =
        serde_yaml::from_str(&yaml_string).context("failed to parse rendered YAML")?;
    Ok(yaml_value)
}

fn append_routing(flow_doc: &mut YamlValue, from_node: &str, to_node: &str) -> Result<()> {
    let nodes = flow_doc
        .as_mapping_mut()
        .and_then(|m| m.get_mut(YamlValue::String("nodes".to_string(), None)))
        .and_then(|n| n.as_mapping_mut())
        .ok_or_else(|| anyhow!("flow missing nodes map"))?;
    let key = YamlValue::String(from_node.to_string(), None);
    let Some(node) = nodes.get_mut(&key) else {
        bail!("node `{from_node}` not found for routing update");
    };
    let mapping = node
        .as_mapping_mut()
        .ok_or_else(|| anyhow!("node `{from_node}` is not a mapping"))?;
    let routes_key = YamlValue::String("routing".to_string(), None);
    let routing = mapping
        .entry(routes_key)
        .or_insert(YamlValue::Sequence(YamlSequence::default()));
    let seq = routing
        .as_sequence_mut()
        .ok_or_else(|| anyhow!("routing for `{from_node}` is not a list"))?;
    let mut entry = YamlMapping::new();
    entry.insert(
        YamlValue::String("to".to_string(), None),
        YamlValue::String(to_node.to_string(), None),
    );
    seq.elements.push(YamlValue::Mapping(entry));
    Ok(())
}
