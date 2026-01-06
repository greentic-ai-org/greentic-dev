use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use greentic_flow::flow_bundle::{blake3_hex, canonicalize_json, load_and_validate_bundle};
use greentic_pack::PackKind;
use greentic_pack::builder::{
    ComponentArtifact, ComponentDescriptor, ComponentPin as PackComponentPin, DistributionSection,
    FlowBundle as PackFlowBundle, ImportRef, NodeRef as PackNodeRef, PACK_VERSION, PackBuilder,
    PackMeta, Provenance, Signing,
};
use greentic_pack::events::EventsSection;
use greentic_pack::messaging::MessagingSection;
use greentic_pack::repo::{InterfaceBinding, RepoPackSection};
use semver::Version;
use semver::VersionReq;
use serde::Deserialize;
use serde_json::{Value as JsonValue, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::component_resolver::{
    ComponentResolver, NodeSchemaError, ResolvedComponent, ResolvedNode,
};
use crate::path_safety::normalize_under_root;

#[derive(Debug, Clone, Copy)]
pub enum PackSigning {
    Dev,
    None,
}

impl From<PackSigning> for Signing {
    fn from(value: PackSigning) -> Self {
        match value {
            PackSigning::Dev => Signing::Dev,
            PackSigning::None => Signing::None,
        }
    }
}

pub fn run(
    flow_path: &Path,
    output_path: &Path,
    signing: PackSigning,
    meta_path: Option<&Path>,
    component_dir: Option<&Path>,
) -> Result<()> {
    let workspace_root = env::current_dir()
        .context("failed to resolve workspace root")?
        .canonicalize()
        .context("failed to canonicalize workspace root")?;
    let safe_flow = normalize_under_root(&workspace_root, flow_path)?;
    let safe_meta = meta_path
        .map(|path| normalize_under_root(&workspace_root, path))
        .transpose()?;
    let safe_component_dir = component_dir
        .map(|dir| normalize_under_root(&workspace_root, dir))
        .transpose()?;

    build_once(
        &safe_flow,
        output_path,
        signing,
        safe_meta.as_deref(),
        safe_component_dir.as_deref(),
    )?;
    if strict_mode_enabled() {
        verify_determinism(
            &safe_flow,
            output_path,
            signing,
            safe_meta.as_deref(),
            safe_component_dir.as_deref(),
        )?;
    }
    Ok(())
}

fn build_once(
    flow_path: &Path,
    output_path: &Path,
    signing: PackSigning,
    meta_path: Option<&Path>,
    component_dir: Option<&Path>,
) -> Result<()> {
    let flow_source = fs::read_to_string(flow_path)
        .with_context(|| format!("failed to read {}", flow_path.display()))?;
    let mut flow_doc_json: JsonValue =
        serde_yaml_bw::from_str(&flow_source).with_context(|| {
            format!(
                "failed to parse {} for node resolution",
                flow_path.display()
            )
        })?;
    let bundle = load_and_validate_bundle(&flow_source, Some(flow_path))
        .with_context(|| format!("flow validation failed for {}", flow_path.display()))?;

    let mut resolver = ComponentResolver::new(component_dir.map(PathBuf::from));
    let mut resolved_nodes = Vec::new();
    let mut schema_errors = Vec::new();

    for node in &bundle.nodes {
        if is_builtin_component(&node.component.name) {
            if node.component.name == "component.exec"
                && let Some(exec_node) =
                    resolve_component_exec_node(&mut resolver, node, &flow_doc_json)?
            {
                schema_errors.extend(resolver.validate_node(&exec_node)?);
                resolved_nodes.push(exec_node);
            }
            continue;
        }
        let resolved = resolver.resolve_node(node, &flow_doc_json)?;
        schema_errors.extend(resolver.validate_node(&resolved)?);
        resolved_nodes.push(resolved);
    }

    if !schema_errors.is_empty() {
        report_schema_errors(&schema_errors)?;
    }

    // Newer runner builds expect node.component.operation to be populated; backfill a default using
    // the first operation declared in the component manifest when the flow omitted it.
    ensure_node_operations(&mut flow_doc_json, &resolved_nodes)?;

    write_resolved_configs(&resolved_nodes)?;

    let meta = load_pack_meta(meta_path, &bundle)?;
    let mut builder = PackBuilder::new(meta)
        .with_flow(to_pack_flow_bundle(&bundle, &flow_doc_json, &flow_source))
        .with_signing(signing.into())
        .with_provenance(build_provenance());

    for artifact in collect_component_artifacts(&resolved_nodes) {
        builder = builder.with_component(artifact);
    }

    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let build_result = builder
        .build(output_path)
        .context("pack build failed (sign/build stage)")?;
    println!(
        "✓ Pack built at {} (manifest hash {})",
        build_result.out_path.display(),
        build_result.manifest_hash_blake3
    );

    Ok(())
}

fn strict_mode_enabled() -> bool {
    matches!(
        std::env::var("LOCAL_CHECK_STRICT")
            .unwrap_or_default()
            .as_str(),
        "1" | "true" | "TRUE"
    )
}

fn verify_determinism(
    flow_path: &Path,
    output_path: &Path,
    signing: PackSigning,
    meta_path: Option<&Path>,
    component_dir: Option<&Path>,
) -> Result<()> {
    let temp_dir = tempfile::tempdir().context("failed to create tempdir for determinism check")?;
    let temp_pack = temp_dir.path().join("deterministic.gtpack");
    build_once(flow_path, &temp_pack, signing, meta_path, component_dir)
        .context("determinism build failed")?;
    let workspace_root = env::current_dir()
        .context("failed to resolve workspace root")?
        .canonicalize()
        .context("failed to canonicalize workspace root")?;
    let safe_output = normalize_under_root(&workspace_root, output_path)?;
    let expected = fs::read(&safe_output).context("failed to read primary pack for determinism")?;
    let actual = fs::read(&temp_pack).context("failed to read temp pack for determinism")?;
    if expected != actual {
        bail!("LOCAL_CHECK_STRICT detected non-deterministic pack output");
    }
    println!("LOCAL_CHECK_STRICT verified deterministic pack output");
    Ok(())
}

fn to_pack_flow_bundle(
    bundle: &greentic_flow::flow_bundle::FlowBundle,
    flow_doc_json: &JsonValue,
    flow_yaml: &str,
) -> PackFlowBundle {
    let canonical_json = canonicalize_json(flow_doc_json);

    PackFlowBundle {
        id: bundle.id.clone(),
        kind: bundle.kind.clone(),
        entry: bundle.entry.clone(),
        yaml: flow_yaml.to_string(),
        json: canonical_json.clone(),
        hash_blake3: blake3_hex(
            serde_json::to_vec(&canonical_json).expect("canonical flow JSON serialization"),
        ),
        nodes: bundle
            .nodes
            .iter()
            .map(|node| PackNodeRef {
                node_id: node.node_id.clone(),
                component: PackComponentPin {
                    name: node.component.name.clone(),
                    version_req: node.component.version_req.clone(),
                },
                schema_id: node.schema_id.clone(),
            })
            .collect(),
    }
}

fn ensure_node_operations(flow_doc_json: &mut JsonValue, nodes: &[ResolvedNode]) -> Result<()> {
    let Some(nodes_map) = flow_doc_json
        .get_mut("nodes")
        .and_then(|v| v.as_object_mut())
    else {
        return Ok(());
    };

    for node in nodes {
        let Some(entry) = nodes_map
            .get_mut(&node.node_id)
            .and_then(|v| v.as_object_mut())
        else {
            continue;
        };
        let Some(config) = entry.get_mut(&node.component.name) else {
            continue;
        };
        let Some(cfg_map) = config.as_object_mut() else {
            continue;
        };

        let has_op = cfg_map
            .get("operation")
            .and_then(|v| v.as_str())
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
            || cfg_map
                .get("op")
                .and_then(|v| v.as_str())
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);

        if has_op {
            continue;
        }

        if let Some(op) = default_operation(&node.component)? {
            cfg_map
                .entry("operation")
                .or_insert(JsonValue::String(op.clone()));
            cfg_map.entry("op").or_insert(JsonValue::String(op));
        }
    }

    Ok(())
}

fn default_operation(component: &ResolvedComponent) -> Result<Option<String>> {
    let manifest_json = component.manifest_json.as_deref().unwrap_or_default();
    let manifest: JsonValue =
        serde_json::from_str(manifest_json).context("invalid manifest JSON")?;
    let op_name = manifest
        .get("operations")
        .and_then(|ops| ops.as_array())
        .and_then(|ops| ops.first())
        .and_then(|op| op.get("name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    Ok(op_name)
}

fn write_resolved_configs(nodes: &[ResolvedNode]) -> Result<()> {
    let root = Path::new(".greentic").join("resolved_config");
    fs::create_dir_all(&root).context("failed to create .greentic/resolved_config")?;
    for node in nodes {
        let path = root.join(format!("{}.json", node.node_id));
        let contents = serde_json::to_string_pretty(&json!({
            "node_id": node.node_id,
            "component": node.component.name,
            "version": node.component.version.to_string(),
            "config": node.config,
        }))?;
        fs::write(&path, contents)
            .with_context(|| format!("failed to write {}", path.display()))?;
    }
    Ok(())
}

fn collect_component_artifacts(nodes: &[ResolvedNode]) -> Vec<ComponentArtifact> {
    let mut map: HashMap<String, ComponentArtifact> = HashMap::new();
    for node in nodes {
        let component = &node.component;
        let key = format!("{}@{}", component.name, component.version);
        map.entry(key).or_insert_with(|| to_artifact(component));
    }
    map.into_values().collect()
}

fn is_builtin_component(name: &str) -> bool {
    name == "component.exec"
        || name == "flow.call"
        || name == "session.wait"
        || name.starts_with("emit")
}

fn resolve_component_exec_node(
    resolver: &mut ComponentResolver,
    node: &greentic_flow::flow_bundle::NodeRef,
    flow_doc_json: &JsonValue,
) -> Result<Option<ResolvedNode>> {
    let nodes = flow_doc_json
        .get("nodes")
        .and_then(|value| value.as_object())
        .ok_or_else(|| anyhow!("flow document missing nodes map"))?;
    let Some(node_value) = nodes.get(&node.node_id) else {
        bail!("node {} missing from flow document", node.node_id);
    };
    let payload = node_value
        .get("component.exec")
        .ok_or_else(|| anyhow!("component.exec payload missing for node {}", node.node_id))?;
    let component_ref = payload
        .get("component")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            anyhow!(
                "component.exec requires `component` for node {}",
                node.node_id
            )
        })?;
    let (name, version_req) = parse_component_ref(component_ref)?;
    let resolved_component = resolver.resolve_component(&name, &version_req)?;
    Ok(Some(ResolvedNode {
        node_id: node.node_id.clone(),
        component: resolved_component,
        pointer: format!("/nodes/{}", node.node_id),
        config: payload.clone(),
    }))
}

fn parse_component_ref(raw: &str) -> Result<(String, VersionReq)> {
    if let Some((name, ver)) = raw.split_once('@') {
        let vr = VersionReq::parse(ver.trim())
            .with_context(|| format!("invalid version requirement `{ver}`"))?;
        Ok((name.trim().to_string(), vr))
    } else {
        Ok((raw.trim().to_string(), VersionReq::default()))
    }
}

fn to_artifact(component: &Arc<ResolvedComponent>) -> ComponentArtifact {
    let hash = component
        .wasm_hash
        .strip_prefix("blake3:")
        .unwrap_or(&component.wasm_hash)
        .to_string();
    ComponentArtifact {
        name: component.name.clone(),
        version: component.version.clone(),
        wasm_path: component.wasm_path.clone(),
        schema_json: component.schema_json.clone(),
        manifest_json: component.manifest_json.clone(),
        capabilities: component.capabilities_json.clone(),
        world: Some(component.world.clone()),
        hash_blake3: Some(hash),
    }
}

fn report_schema_errors(errors: &[NodeSchemaError]) -> Result<()> {
    let mut message = String::new();
    for err in errors {
        message.push_str(&format!(
            "- node `{}` ({}) {}: {}\n",
            err.node_id, err.component, err.pointer, err.message
        ));
    }
    bail!("component schema validation failed:\n{message}");
}

fn load_pack_meta(
    meta_path: Option<&Path>,
    bundle: &greentic_flow::flow_bundle::FlowBundle,
) -> Result<PackMeta> {
    let config = if let Some(path) = meta_path {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        toml::from_str::<PackMetaToml>(&raw)
            .with_context(|| format!("invalid pack metadata {}", path.display()))?
    } else {
        PackMetaToml::default()
    };

    let pack_id = config
        .pack_id
        .unwrap_or_else(|| format!("dev.local.{}", bundle.id));
    let version = config
        .version
        .as_deref()
        .unwrap_or("0.1.0")
        .parse::<Version>()
        .context("invalid pack version in metadata")?;
    let pack_version = config.pack_version.unwrap_or(PACK_VERSION);
    let name = config.name.unwrap_or_else(|| bundle.id.clone());
    let description = config.description;
    let authors = config.authors.unwrap_or_default();
    let license = config.license;
    let homepage = config.homepage;
    let support = config.support;
    let vendor = config.vendor;
    let kind = config.kind;
    let events = config.events;
    let repo = config.repo;
    let messaging = config.messaging;
    let interfaces = config.interfaces.unwrap_or_default();
    let imports = config
        .imports
        .unwrap_or_default()
        .into_iter()
        .map(|imp| ImportRef {
            pack_id: imp.pack_id,
            version_req: imp.version_req,
        })
        .collect();
    let entry_flows = config
        .entry_flows
        .unwrap_or_else(|| vec![bundle.id.clone()]);
    let created_at_utc = config.created_at_utc.unwrap_or_else(|| {
        OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_default()
    });
    let annotations = config.annotations.map(toml_to_json_map).unwrap_or_default();
    let distribution = config.distribution;
    let components = config.components.unwrap_or_default();

    Ok(PackMeta {
        pack_version,
        pack_id,
        version,
        name,
        description,
        authors,
        license,
        homepage,
        support,
        vendor,
        imports,
        kind,
        entry_flows,
        created_at_utc,
        events,
        repo,
        messaging,
        interfaces,
        annotations,
        distribution,
        components,
    })
}

fn toml_to_json_map(table: toml::value::Table) -> serde_json::Map<String, JsonValue> {
    table
        .into_iter()
        .map(|(key, value)| {
            let json_value: JsonValue = value.try_into().unwrap_or(JsonValue::Null);
            (key, json_value)
        })
        .collect()
}

fn build_provenance() -> Provenance {
    Provenance {
        builder: format!("greentic-dev {}", env!("CARGO_PKG_VERSION")),
        git_commit: git_rev().ok(),
        git_repo: git_remote().ok(),
        toolchain: None,
        built_at_utc: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "unknown".into()),
        host: std::env::var("HOSTNAME").ok(),
        notes: Some("Built via greentic-dev pack build".into()),
    }
}

fn git_rev() -> Result<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !output.status.success() {
        bail!("git rev-parse failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_remote() -> Result<String> {
    let output = std::process::Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .output()?;
    if !output.status.success() {
        bail!("git remote lookup failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[derive(Debug, Deserialize, Default)]
struct PackMetaToml {
    pack_version: Option<u32>,
    pack_id: Option<String>,
    version: Option<String>,
    name: Option<String>,
    kind: Option<PackKind>,
    description: Option<String>,
    authors: Option<Vec<String>>,
    license: Option<String>,
    homepage: Option<String>,
    support: Option<String>,
    vendor: Option<String>,
    entry_flows: Option<Vec<String>>,
    events: Option<EventsSection>,
    repo: Option<RepoPackSection>,
    messaging: Option<MessagingSection>,
    interfaces: Option<Vec<InterfaceBinding>>,
    imports: Option<Vec<ImportToml>>,
    annotations: Option<toml::value::Table>,
    created_at_utc: Option<String>,
    distribution: Option<DistributionSection>,
    components: Option<Vec<ComponentDescriptor>>,
}

#[derive(Debug, Deserialize)]
struct ImportToml {
    pack_id: String,
    version_req: String,
}
