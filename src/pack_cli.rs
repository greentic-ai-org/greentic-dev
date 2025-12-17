use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result, anyhow};
use greentic_pack::builder::PackManifest;
use greentic_pack::events::EventProviderSpec;
use greentic_pack::plan::infer_base_deployment_plan;
use greentic_pack::reader::{SigningPolicy, open_pack};
use greentic_types::SecretRequirement;
use greentic_types::component::ComponentManifest;
use greentic_types::{EnvId, TenantCtx, TenantId};
use serde_json::json;
use zip::ZipArchive;

use crate::cli::{PackEventsFormatArg, PackEventsListArgs, PackPlanArgs, PackPolicyArg};
use crate::pack_temp::materialize_pack_path;

#[derive(Copy, Clone, Debug)]
pub enum PackEventsFormat {
    Table,
    Json,
    Yaml,
}

impl From<PackEventsFormatArg> for PackEventsFormat {
    fn from(value: PackEventsFormatArg) -> Self {
        match value {
            PackEventsFormatArg::Table => PackEventsFormat::Table,
            PackEventsFormatArg::Json => PackEventsFormat::Json,
            PackEventsFormatArg::Yaml => PackEventsFormat::Yaml,
        }
    }
}

impl From<PackPolicyArg> for SigningPolicy {
    fn from(value: PackPolicyArg) -> Self {
        match value {
            PackPolicyArg::Devok => SigningPolicy::DevOk,
            PackPolicyArg::Strict => SigningPolicy::Strict,
        }
    }
}

pub fn pack_inspect(path: &Path, policy: PackPolicyArg, json: bool) -> Result<()> {
    let (temp, pack_path) = materialize_pack_path(path, false)?;
    let load = open_pack(&pack_path, policy.into()).map_err(|err| anyhow!(err.message))?;
    if json {
        print_inspect_json(&load.manifest, &load.report, &load.sbom)?;
    } else {
        print_inspect_human(&load.manifest, &load.report, &load.sbom);
    }
    drop(temp);
    Ok(())
}

pub fn pack_plan(args: &PackPlanArgs) -> Result<()> {
    let (temp, pack_path) = materialize_pack_path(&args.input, args.verbose)?;
    let tenant_ctx = build_tenant_ctx(&args.environment, &args.tenant)?;
    let plan = plan_for_pack(&pack_path, &tenant_ctx, &args.environment)?;

    if args.json {
        println!("{}", serde_json::to_string(&plan)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&plan)?);
    }

    drop(temp);
    Ok(())
}

pub fn pack_events_list(args: &PackEventsListArgs) -> Result<()> {
    let (temp, pack_path) = materialize_pack_path(&args.path, args.verbose)?;
    let load = open_pack(&pack_path, SigningPolicy::DevOk).map_err(|err| anyhow!(err.message))?;
    let providers: Vec<EventProviderSpec> = load
        .manifest
        .meta
        .events
        .as_ref()
        .map(|events| events.providers.clone())
        .unwrap_or_default();

    match PackEventsFormat::from(args.format) {
        PackEventsFormat::Table => print_table(&providers),
        PackEventsFormat::Json => print_json(&providers)?,
        PackEventsFormat::Yaml => print_yaml(&providers)?,
    }

    drop(temp);
    Ok(())
}

fn plan_for_pack(
    path: &Path,
    tenant: &TenantCtx,
    environment: &str,
) -> Result<greentic_types::deployment::DeploymentPlan> {
    let load = open_pack(path, SigningPolicy::DevOk).map_err(|err| anyhow!(err.message))?;
    let connectors = load.manifest.meta.annotations.get("connectors");
    let components = load_component_manifests(path, &load.manifest)?;
    let secret_requirements = load_secret_requirements(path)?;

    Ok(infer_base_deployment_plan(
        &load.manifest.meta,
        &load.manifest.flows,
        connectors,
        &components,
        secret_requirements,
        tenant,
        environment,
    ))
}

fn build_tenant_ctx(environment: &str, tenant: &str) -> Result<TenantCtx> {
    let env_id = EnvId::from_str(environment)
        .with_context(|| format!("invalid environment id `{}`", environment))?;
    let tenant_id =
        TenantId::from_str(tenant).with_context(|| format!("invalid tenant id `{}`", tenant))?;
    Ok(TenantCtx::new(env_id, tenant_id))
}

fn load_component_manifests(
    pack_path: &Path,
    pack_manifest: &PackManifest,
) -> Result<HashMap<String, ComponentManifest>> {
    let file =
        File::open(pack_path).with_context(|| format!("failed to open {}", pack_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("{} is not a valid gtpack archive", pack_path.display()))?;

    let mut manifests = HashMap::new();
    for component in &pack_manifest.components {
        if let Some(manifest_path) = component.manifest_file.as_deref() {
            let mut entry = archive
                .by_name(manifest_path)
                .with_context(|| format!("component manifest `{}` missing", manifest_path))?;
            let manifest: ComponentManifest =
                serde_json::from_reader(&mut entry).with_context(|| {
                    format!("failed to parse component manifest `{}`", manifest_path)
                })?;
            manifests.insert(component.name.clone(), manifest);
        }
    }

    Ok(manifests)
}

fn load_secret_requirements(path: &Path) -> Result<Option<Vec<SecretRequirement>>> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("{} is not a valid gtpack archive", path.display()))?;

    for name in [
        "assets/secret-requirements.json",
        "secret-requirements.json",
    ] {
        if let Ok(mut entry) = archive.by_name(name) {
            let mut buf = String::new();
            entry
                .read_to_string(&mut buf)
                .context("failed to read secret requirements file")?;
            let reqs: Vec<SecretRequirement> =
                serde_json::from_str(&buf).context("secret requirements file is invalid JSON")?;
            return Ok(Some(reqs));
        }
    }

    Ok(None)
}

fn print_inspect_human(
    manifest: &PackManifest,
    report: &greentic_pack::reader::VerifyReport,
    sbom: &[greentic_pack::builder::SbomEntry],
) {
    println!(
        "Pack: {} ({})",
        manifest.meta.pack_id, manifest.meta.version
    );
    println!("Flows: {}", manifest.flows.len());
    println!("Components: {}", manifest.components.len());
    println!("SBOM entries: {}", sbom.len());
    println!("Signature OK: {}", report.signature_ok);
    println!("SBOM OK: {}", report.sbom_ok);
    if report.warnings.is_empty() {
        println!("Warnings: none");
    } else {
        println!("Warnings:");
        for warning in &report.warnings {
            println!("  - {}", warning);
        }
    }
}

fn print_inspect_json(
    manifest: &PackManifest,
    report: &greentic_pack::reader::VerifyReport,
    sbom: &[greentic_pack::builder::SbomEntry],
) -> Result<()> {
    let payload = json!({
        "manifest": {
            "pack_id": manifest.meta.pack_id,
            "version": manifest.meta.version,
            "flows": manifest.flows.len(),
            "components": manifest.components.len(),
        },
        "report": {
            "signature_ok": report.signature_ok,
            "sbom_ok": report.sbom_ok,
            "warnings": report.warnings,
        },
        "sbom": sbom,
    });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

fn print_table(providers: &[EventProviderSpec]) {
    if providers.is_empty() {
        println!("No events providers declared.");
        return;
    }

    println!(
        "{:<20} {:<8} {:<28} {:<12} TOPICS",
        "NAME", "KIND", "COMPONENT", "TRANSPORT"
    );
    for provider in providers {
        let transport = provider
            .capabilities
            .transport
            .as_ref()
            .map(|t| t.to_string())
            .unwrap_or_else(|| "-".to_string());
        let topics = summarize_topics(&provider.capabilities.topics);
        println!(
            "{:<20} {:<8} {:<28} {:<12} {}",
            provider.name, provider.kind, provider.component, transport, topics
        );
    }
}

fn print_json(providers: &[EventProviderSpec]) -> Result<()> {
    let payload = json!(providers);
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

fn print_yaml(providers: &[EventProviderSpec]) -> Result<()> {
    let doc = serde_yaml_bw::to_string(providers)?;
    println!("{doc}");
    Ok(())
}

fn summarize_topics(topics: &[String]) -> String {
    if topics.is_empty() {
        return "-".to_string();
    }
    let combined = topics.join(", ");
    if combined.len() > 60 {
        format!("{}...", &combined[..57])
    } else {
        combined
    }
}
