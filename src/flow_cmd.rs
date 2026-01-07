use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::{ConfigFlowModeArg, FlowAddStepArgs};
use crate::component_add::run_component_add;
use crate::pack_init::PackInitIntent;
use crate::path_safety::normalize_under_root;
use anyhow::{Context, Result, anyhow, bail};
use greentic_flow::add_step::{add_step_from_config_flow, anchor_candidates};
use greentic_flow::component_catalog::normalize_manifest_value;
use greentic_flow::flow_bundle::load_and_validate_bundle;
use greentic_flow::flow_ir::FlowIr;
use greentic_flow::loader::load_ygtc_from_str;
use serde_json::Value as JsonValue;
use serde_yaml_bw as serde_yaml;
use std::io::Write;
use std::str::FromStr;
use std::{io, io::IsTerminal};
use tempfile::NamedTempFile;

use greentic_types::FlowId;
use greentic_types::component::ComponentManifest;

const FLOW_SCHEMA: &str = include_str!("../schemas/ygtc.flow.schema.json");

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
    let mut manifest_value: JsonValue = serde_json::from_str(&manifest_raw).with_context(|| {
        format!(
            "failed to parse component manifest JSON at {}",
            manifest_path.display()
        )
    })?;
    normalize_manifest_value(&mut manifest_value);
    let manifest: ComponentManifest =
        serde_json::from_value(manifest_value).with_context(|| {
            format!(
                "failed to parse component manifest JSON at {}",
                manifest_path.display()
            )
        })?;

    let config_flow_id = match args.mode {
        Some(ConfigFlowModeArg::Custom) => "custom".to_string(),
        Some(ConfigFlowModeArg::Default) => "default".to_string(),
        None => args.flow.clone(),
    };
    let config_flow_key = FlowId::from_str(&config_flow_id).map_err(|_| {
        anyhow!(
            "invalid flow identifier `{}`; flow ids must be valid FlowId strings",
            config_flow_id
        )
    })?;
    let Some(config_flow) = manifest.dev_flows.get(&config_flow_key) else {
        bail!(
            "Flow '{}' is missing from manifest.dev_flows. Run `greentic-component flow update` to regenerate config flows.",
            config_flow_id
        );
    };

    let coord = args
        .coordinate
        .ok_or_else(|| anyhow!("component coordinate is required (pass --coordinate)"))?;

    // Ensure the component is available locally (fetch if needed).
    let _bundle_dir = resolve_component_bundle(&coord, args.profile.as_deref())?;

    // Render the dev flow graph to YAML so greentic-flow helpers can consume it (type defaulting is handled upstream).
    let config_flow_yaml =
        serde_yaml::to_string(&config_flow.graph).context("failed to render config flow graph")?;
    let mut temp_flow =
        NamedTempFile::new().context("failed to create temporary config flow file")?;
    temp_flow
        .write_all(config_flow_yaml.as_bytes())
        .context("failed to write temporary config flow")?;
    temp_flow.flush()?;

    let pack_flow_path = PathBuf::from("flows").join(format!("{}.ygtc", args.flow_id));
    if !pack_flow_path.exists() {
        bail!(
            "Pack flow '{}' not found at {}",
            args.flow_id,
            pack_flow_path.display()
        );
    }
    let pack_flow_raw = std::fs::read_to_string(&pack_flow_path)
        .with_context(|| format!("failed to read pack flow {}", pack_flow_path.display()))?;
    let flow_doc = load_ygtc_from_str(&pack_flow_raw)
        .with_context(|| format!("failed to parse flow at {}", pack_flow_path.display()))?;
    let flow_ir = FlowIr::from_doc(flow_doc).context("failed to convert flow document to IR")?;
    let schema_path = PathBuf::from(".greentic").join("config-flow.schema.json");
    if let Some(parent) = schema_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&schema_path, FLOW_SCHEMA)
        .with_context(|| format!("failed to write {}", schema_path.display()))?;

    let after = args
        .after
        .clone()
        .or_else(|| prompt_routing_target(&flow_ir));
    let answers = serde_json::Map::new();
    let updated_doc = add_step_from_config_flow(
        &pack_flow_raw,
        temp_flow.path(),
        &schema_path,
        &[manifest_path.as_path()],
        after,
        &answers,
        false,
    )
    .map_err(|e| anyhow!(e))?;

    let rendered =
        serde_yaml::to_string(&updated_doc).context("failed to render updated pack flow")?;
    std::fs::write(&pack_flow_path, rendered)
        .with_context(|| format!("failed to write {}", pack_flow_path.display()))?;

    println!(
        "Added node from config flow {config_flow_id} to {}",
        pack_flow_path.display()
    );
    Ok(())
}

fn prompt_routing_target(flow_ir: &FlowIr) -> Option<String> {
    if !io::stdout().is_terminal() {
        return None;
    }
    let keys = anchor_candidates(flow_ir);
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

fn resolve_component_bundle(coordinate: &str, profile: Option<&str>) -> Result<PathBuf> {
    let path = PathBuf::from_str(coordinate).unwrap_or_default();
    if path.exists() {
        return Ok(path);
    }
    let dir = run_component_add(coordinate, profile, PackInitIntent::Dev)?;
    Ok(dir)
}
