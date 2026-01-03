use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result, anyhow};
use greentic_pack::builder::PackManifest;
use greentic_pack::events::EventProviderSpec;
use greentic_pack::plan::infer_base_deployment_plan;
use greentic_pack::reader::{SigningPolicy, open_pack};
use greentic_types::ExtensionRef;
use greentic_types::SecretRequirement;
use greentic_types::component::ComponentManifest;
use greentic_types::provider::{
    ProviderDecl, ProviderExtensionInline, ProviderManifest, ProviderRuntimeRef,
};
use greentic_types::{EnvId, TenantCtx, TenantId};
use serde_json::json;
use zip::ZipArchive;

use crate::cli::{
    PackEventsFormatArg, PackEventsListArgs, PackNewProviderArgs, PackPlanArgs, PackPolicyArg,
};
use crate::pack_init::slugify;
use crate::pack_temp::materialize_pack_path;

const PROVIDER_EXTENSION_ID: &str = "greentic.provider-extension.v1";

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

pub fn pack_new_provider(args: &PackNewProviderArgs) -> Result<()> {
    let (mut manifest, location, pack_root) = load_manifest(&args.pack)?;

    let runtime = parse_runtime_ref(&args.runtime)?;
    let config_ref = args
        .manifest
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| format!("providers/{}/provider.yaml", slugify(&args.id)));

    let mut decl = ProviderDecl {
        provider_type: args.id.clone(),
        capabilities: Vec::new(),
        ops: Vec::new(),
        config_schema_ref: config_ref.clone(),
        state_schema_ref: None,
        runtime,
        docs_ref: None,
    };
    if let Some(kind) = &args.kind {
        decl.capabilities.push(kind.clone());
    }

    let mut inline = load_provider_extension(&manifest)?;
    if let Some(existing) = inline
        .providers
        .iter()
        .position(|p| p.provider_type == args.id)
    {
        if !args.force {
            anyhow::bail!(
                "provider `{}` already exists; pass --force to update",
                args.id
            );
        }
        inline.providers.remove(existing);
    }
    inline.providers.push(decl.clone());
    inline
        .providers
        .sort_by(|a, b| a.provider_type.cmp(&b.provider_type));
    validate_provider_extension(&inline)?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&decl)?);
    }

    if !args.dry_run {
        set_provider_extension(&mut manifest, &inline)?;
        write_manifest(location, &manifest)?;
        if args.scaffold_files {
            scaffold_provider_manifest(&pack_root, &config_ref, &decl)?;
        }
    }

    Ok(())
}

fn scaffold_provider_manifest(
    pack_root: &Path,
    manifest_ref: &str,
    decl: &ProviderDecl,
) -> Result<()> {
    let path = pack_root.join(manifest_ref);
    let parent = path
        .parent()
        .with_context(|| format!("cannot derive parent for {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let provider_manifest = ProviderManifest {
        provider_type: decl.provider_type.clone(),
        capabilities: decl.capabilities.clone(),
        ops: decl.ops.clone(),
        config_schema_ref: Some(decl.config_schema_ref.clone()),
        state_schema_ref: decl.state_schema_ref.clone(),
    };
    let serialized = serde_yaml_bw::to_string(&provider_manifest)?;
    fs::write(&path, serialized).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn parse_runtime_ref(input: &str) -> Result<ProviderRuntimeRef> {
    let (left, world) = input
        .rsplit_once('@')
        .context("runtime must be in form component_ref::export@world")?;
    let (component_ref, export) = left
        .split_once("::")
        .context("runtime must be in form component_ref::export@world")?;
    Ok(ProviderRuntimeRef {
        component_ref: component_ref.to_string(),
        export: export.to_string(),
        world: world.to_string(),
    })
}

fn load_provider_extension(
    manifest: &greentic_types::PackManifest,
) -> Result<ProviderExtensionInline> {
    let mut inline = ProviderExtensionInline::default();
    if let Some(inline_ref) = manifest
        .extensions
        .as_ref()
        .and_then(|exts| exts.get(PROVIDER_EXTENSION_ID))
        .and_then(|ext| ext.inline.as_ref())
    {
        inline = match inline_ref {
            greentic_types::pack_manifest::ExtensionInline::Provider(value) => value.clone(),
            greentic_types::pack_manifest::ExtensionInline::Other(value) => {
                serde_json::from_value(value.clone()).unwrap_or_default()
            }
        };
    }
    Ok(inline)
}

fn set_provider_extension(
    manifest: &mut greentic_types::PackManifest,
    inline: &ProviderExtensionInline,
) -> Result<()> {
    let extensions = manifest.extensions.get_or_insert_with(Default::default);
    let entry = extensions
        .entry(PROVIDER_EXTENSION_ID.to_string())
        .or_insert_with(|| ExtensionRef {
            kind: PROVIDER_EXTENSION_ID.to_string(),
            version: "1.0.0".to_string(),
            digest: None,
            location: None,
            inline: None,
        });
    entry.inline = Some(greentic_types::pack_manifest::ExtensionInline::Provider(
        inline.clone(),
    ));
    Ok(())
}

fn validate_provider_extension(inline: &ProviderExtensionInline) -> Result<()> {
    let mut seen = HashSet::new();
    for provider in &inline.providers {
        if provider.provider_type.trim().is_empty() {
            anyhow::bail!("provider_type must not be empty");
        }
        if !seen.insert(provider.provider_type.as_str()) {
            anyhow::bail!("duplicate provider_type `{}`", provider.provider_type);
        }
        if provider.runtime.component_ref.trim().is_empty()
            || provider.runtime.export.trim().is_empty()
            || provider.runtime.world.trim().is_empty()
        {
            anyhow::bail!(
                "runtime fields must be set for provider `{}`",
                provider.provider_type
            );
        }
    }
    Ok(())
}

enum ManifestLocation {
    File(PathBuf),
    Gtpack(PathBuf),
}

fn load_manifest(path: &Path) -> Result<(greentic_types::PackManifest, ManifestLocation, PathBuf)> {
    if path.is_dir() {
        let dist = path.join("dist/manifest.cbor");
        let root_manifest = path.join("manifest.cbor");
        let target = if dist.exists() {
            dist
        } else if root_manifest.exists() {
            root_manifest
        } else {
            anyhow::bail!(
                "pack path {} is a directory but manifest.cbor not found (looked in ./dist/ and root)",
                path.display()
            );
        };
        let bytes = fs::read(&target)
            .with_context(|| format!("failed to read manifest {}", target.display()))?;
        let manifest = greentic_types::decode_pack_manifest(&bytes)?;
        return Ok((manifest, ManifestLocation::File(target), path.to_path_buf()));
    }

    if path.extension().is_some_and(|ext| ext == "gtpack") {
        let mut archive = zip::ZipArchive::new(File::open(path).context("open gtpack")?)
            .context("read gtpack zip")?;
        let mut manifest_bytes = Vec::new();
        archive
            .by_name("manifest.cbor")
            .context("manifest.cbor missing in gtpack")?
            .read_to_end(&mut manifest_bytes)
            .context("read manifest.cbor")?;
        let manifest = greentic_types::decode_pack_manifest(&manifest_bytes)?;
        return Ok((
            manifest,
            ManifestLocation::Gtpack(path.to_path_buf()),
            path.parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(".")),
        ));
    }

    let bytes =
        fs::read(path).with_context(|| format!("failed to read manifest {}", path.display()))?;
    let manifest = greentic_types::decode_pack_manifest(&bytes)?;
    let parent = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Ok((manifest, ManifestLocation::File(path.to_path_buf()), parent))
}

fn write_manifest(
    location: ManifestLocation,
    manifest: &greentic_types::PackManifest,
) -> Result<()> {
    let encoded = greentic_types::encode_pack_manifest(manifest)?;
    match location {
        ManifestLocation::File(path) => {
            fs::write(&path, encoded).with_context(|| format!("write {}", path.display()))?;
        }
        ManifestLocation::Gtpack(path) => {
            let mut archive =
                zip::ZipArchive::new(File::open(&path).context("open gtpack for write")?)
                    .context("read gtpack zip")?;
            let mut entries = Vec::new();
            for i in 0..archive.len() {
                let mut file = archive.by_index(i).context("gtpack entry")?;
                let mut buf = Vec::new();
                file.read_to_end(&mut buf)
                    .with_context(|| format!("read {}", file.name()))?;
                entries.push((file.name().to_string(), buf, file.compression()));
            }
            let temp_path = path.with_extension("gtpack.tmp");
            {
                let temp_file = File::create(&temp_path)
                    .with_context(|| format!("create {}", temp_path.display()))?;
                let mut writer = zip::ZipWriter::new(temp_file);
                let opts = zip::write::SimpleFileOptions::default();
                for (name, data, method) in entries {
                    let mut entry_opts = opts;
                    entry_opts = entry_opts.compression_method(method);
                    if name == "manifest.cbor" {
                        writer
                            .start_file(name, entry_opts)
                            .context("start manifest entry")?;
                        writer.write_all(&encoded).context("write manifest.cbor")?;
                    } else {
                        writer
                            .start_file(name, entry_opts)
                            .with_context(|| "start entry")?;
                        writer.write_all(&data).with_context(|| "write entry")?;
                    }
                }
                writer.finish().context("finish gtpack rewrite")?;
            }
            fs::rename(&temp_path, &path).with_context(|| format!("replace {}", path.display()))?;
        }
    }
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
