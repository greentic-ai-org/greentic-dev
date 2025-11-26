use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use greentic_flow::flow_bundle::load_and_validate_bundle;
use greentic_pack::builder::{
    ComponentArtifact, ComponentPin as PackComponentPin, FlowBundle as PackFlowBundle, ImportRef,
    NodeRef as PackNodeRef, PackBuilder, PackMeta, Provenance, Signing,
};
use greentic_pack::events::EventsSection;
use greentic_pack::messaging::MessagingSection;
use greentic_pack::repo::RepoPackSection;
use greentic_types::PackKind;
use semver::Version;
use serde::Deserialize;
use serde_json::{Value as JsonValue, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::component_resolver::{
    ComponentResolver, NodeSchemaError, ResolvedComponent, ResolvedNode,
};

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
    build_once(flow_path, output_path, signing, meta_path, component_dir)?;
    if strict_mode_enabled() {
        verify_determinism(flow_path, output_path, signing, meta_path, component_dir)?;
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
    let flow_doc_json: JsonValue = serde_yaml_bw::from_str(&flow_source).with_context(|| {
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
        let resolved = resolver.resolve_node(node, &flow_doc_json)?;
        schema_errors.extend(resolver.validate_node(&resolved)?);
        resolved_nodes.push(resolved);
    }

    if !schema_errors.is_empty() {
        report_schema_errors(&schema_errors)?;
    }

    write_resolved_configs(&resolved_nodes)?;

    let meta = load_pack_meta(meta_path, &bundle)?;
    let mut builder = PackBuilder::new(meta)
        .with_flow(to_pack_flow_bundle(&bundle))
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
    let expected = fs::read(output_path).context("failed to read primary pack for determinism")?;
    let actual = fs::read(&temp_pack).context("failed to read temp pack for determinism")?;
    if expected != actual {
        bail!("LOCAL_CHECK_STRICT detected non-deterministic pack output");
    }
    println!("LOCAL_CHECK_STRICT verified deterministic pack output");
    Ok(())
}

fn to_pack_flow_bundle(bundle: &greentic_flow::flow_bundle::FlowBundle) -> PackFlowBundle {
    PackFlowBundle {
        id: bundle.id.clone(),
        kind: bundle.kind.clone(),
        entry: bundle.entry.clone(),
        yaml: bundle.yaml.clone(),
        json: bundle.json.clone(),
        hash_blake3: bundle.hash_blake3.clone(),
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
    let name = config.name.unwrap_or_else(|| bundle.id.clone());
    let description = config.description;
    let authors = config.authors.unwrap_or_default();
    let license = config.license;
    let kind = config.kind;
    let events = config.events;
    let repo = config.repo;
    let messaging = config.messaging;
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

    Ok(PackMeta {
        pack_id,
        version,
        name,
        description,
        authors,
        license,
        imports,
        kind,
        entry_flows,
        created_at_utc,
        events,
        repo,
        messaging,
        annotations,
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
    pack_id: Option<String>,
    version: Option<String>,
    name: Option<String>,
    kind: Option<PackKind>,
    description: Option<String>,
    authors: Option<Vec<String>>,
    license: Option<String>,
    entry_flows: Option<Vec<String>>,
    events: Option<EventsSection>,
    repo: Option<RepoPackSection>,
    messaging: Option<MessagingSection>,
    imports: Option<Vec<ImportToml>>,
    annotations: Option<toml::value::Table>,
    created_at_utc: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ImportToml {
    pack_id: String,
    version_req: String,
}
