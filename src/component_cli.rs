use std::collections::{BTreeSet, HashMap};
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand};
use component_runtime::{
    self, Bindings, ComponentManifestInfo, ComponentRef, HostPolicy, LoadPolicy,
};
use component_store::ComponentStore;
use convert_case::{Case, Casing};
use greentic_types::{EnvId, TenantCtx as FlowTenantCtx, TenantId};
use greentic_types_compat::TenantCtx as RuntimeTenantCtx;
use once_cell::sync::Lazy;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use wit_component::{DecodedWasm, decode as decode_component};
use wit_parser::{Resolve, WorldId, WorldItem};

static WORKSPACE_ROOT: Lazy<PathBuf> = Lazy::new(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));

const TEMPLATE_COMPONENT_CARGO: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/component/Cargo.toml"
));
const TEMPLATE_SRC_LIB: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/component/src/lib.rs"
));
const TEMPLATE_PROVIDER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/component/provider.toml"
));
const TEMPLATE_SCHEMA_CONFIG: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/component/schemas/v1/config.schema.json"
));
const TEMPLATE_README: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/component/README.md"
));
const TEMPLATE_WORLD: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/component/wit/world.wit"
));

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProviderMetadata {
    name: String,
    version: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    homepage: Option<String>,
    abi: AbiSection,
    capabilities: CapabilitiesSection,
    exports: ExportsSection,
    #[serde(default)]
    imports: ImportsSection,
    artifact: ArtifactSection,
    #[serde(default)]
    docs: Option<DocsSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AbiSection {
    interfaces_version: String,
    types_version: String,
    component_runtime: String,
    world: String,
    #[serde(default)]
    wit_packages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CapabilitiesSection {
    #[serde(default)]
    secrets: bool,
    #[serde(default)]
    telemetry: bool,
    #[serde(default)]
    network: bool,
    #[serde(default)]
    filesystem: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExportsSection {
    #[serde(default)]
    provides: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ImportsSection {
    #[serde(default)]
    requires: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArtifactSection {
    format: String,
    path: String,
    #[serde(default)]
    sha256: String,
    #[serde(default)]
    created: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DocsSection {
    #[serde(default)]
    readme: Option<String>,
    #[serde(default)]
    schemas: Vec<String>,
}

#[derive(Debug)]
struct ValidationReport {
    provider: ProviderMetadata,
    component_dir: PathBuf,
    artifact_path: PathBuf,
    sha256: String,
    world: String,
    packages: Vec<String>,
    manifest: Option<ComponentManifestInfo>,
}

#[derive(Debug, Clone)]
struct WitInfo {
    version: String,
    dir: PathBuf,
}

#[derive(Debug, Clone)]
struct Versions {
    interfaces: String,
    types: String,
    component_runtime: String,
    component_wit: WitInfo,
    host_import_wit: WitInfo,
    types_core_wit: WitInfo,
}

impl Versions {
    fn load() -> Result<Self> {
        let interfaces_version = resolved_version("greentic-interfaces")?;
        let types_version = resolved_version("greentic-types")?;
        let component_runtime_version = resolved_version("component-runtime")?;

        let interfaces_root = find_crate_source("greentic-interfaces", &interfaces_version)?;
        let component_wit = detect_wit_package(&interfaces_root, "component")?;
        let host_import_wit = detect_wit_package(&interfaces_root, "host-import")?;
        let types_core_wit = detect_wit_package(&interfaces_root, "types-core")?;

        Ok(Self {
            interfaces: interfaces_version,
            types: types_version,
            component_runtime: component_runtime_version,
            component_wit,
            host_import_wit,
            types_core_wit,
        })
    }
}

static VERSIONS: Lazy<Versions> =
    Lazy::new(|| Versions::load().expect("load greentic crate versions"));

pub fn run_component_command(command: ComponentCommands) -> Result<()> {
    match command {
        ComponentCommands::New(args) => new_component(args),
        ComponentCommands::Validate(args) => validate_command(args),
        ComponentCommands::Pack(args) => pack_command(args),
        ComponentCommands::DemoRun(args) => demo_run_command(args),
    }
}

#[derive(Subcommand, Debug, Clone)]
pub enum ComponentCommands {
    /// Scaffold a new component repository
    New(NewComponentArgs),
    /// Build and validate a component against pinned interfaces
    Validate(ValidateArgs),
    /// Package a component into `packs/<name>/<version>`
    Pack(PackArgs),
    /// Execute a component locally with default mocks
    DemoRun(DemoRunArgs),
}

#[derive(Args, Debug, Clone)]
pub struct NewComponentArgs {
    /// Name of the component (kebab-case recommended)
    name: String,
    /// Optional directory where the component should be created
    #[arg(long, value_name = "DIR")]
    dir: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct ValidateArgs {
    /// Path to the component directory
    #[arg(long, value_name = "PATH", default_value = ".")]
    path: PathBuf,
    /// Skip cargo component build (use the existing artifact)
    #[arg(long)]
    skip_build: bool,
}

#[derive(Args, Debug, Clone)]
pub struct PackArgs {
    /// Path to the component directory
    #[arg(long, value_name = "PATH", default_value = ".")]
    path: PathBuf,
    /// Output directory for generated packs (defaults to `<component>/packs`)
    #[arg(long, value_name = "DIR")]
    out_dir: Option<PathBuf>,
    /// Skip cargo component build before packing
    #[arg(long)]
    skip_build: bool,
}

#[derive(Args, Debug, Clone)]
pub struct DemoRunArgs {
    /// Path to the component directory
    #[arg(long, value_name = "PATH", default_value = ".")]
    path: PathBuf,
    /// Optional path to the component artifact to execute
    #[arg(long, value_name = "FILE")]
    artifact: Option<PathBuf>,
    /// Operation to invoke (defaults to "invoke")
    #[arg(long, value_name = "NAME")]
    operation: Option<String>,
    /// JSON string payload for the invoke call
    #[arg(long, value_name = "JSON")]
    input: Option<String>,
    /// Path to a JSON file with configuration used for binding
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,
    /// Skip rebuilding the component before running
    #[arg(long)]
    skip_build: bool,
}

pub fn new_component(args: NewComponentArgs) -> Result<()> {
    let context = TemplateContext::new(&args.name)?;
    let base_dir = match args.dir {
        Some(ref dir) if dir.is_absolute() => dir.clone(),
        Some(dir) => env::current_dir()
            .with_context(|| "failed to resolve current directory")?
            .join(dir),
        None => env::current_dir().with_context(|| "failed to resolve current directory")?,
    };
    fs::create_dir_all(&base_dir)
        .with_context(|| format!("failed to prepare base directory {}", base_dir.display()))?;
    let component_dir = base_dir.join(context.component_dir());

    if component_dir.exists() {
        bail!(
            "component directory `{}` already exists",
            component_dir.display()
        );
    }

    println!(
        "Creating new component scaffold at `{}`",
        component_dir.display()
    );

    create_dir(component_dir.join("src"))?;
    create_dir(component_dir.join("schemas/v1"))?;
    create_dir(component_dir.join("wit/deps"))?;

    write_template(
        &component_dir.join("Cargo.toml"),
        TEMPLATE_COMPONENT_CARGO,
        &context,
    )?;
    write_template(&component_dir.join("README.md"), TEMPLATE_README, &context)?;
    write_template(
        &component_dir.join("provider.toml"),
        TEMPLATE_PROVIDER,
        &context,
    )?;
    write_template(
        &component_dir.join("src/lib.rs"),
        TEMPLATE_SRC_LIB,
        &context,
    )?;
    write_template(
        &component_dir.join("schemas/v1/config.schema.json"),
        TEMPLATE_SCHEMA_CONFIG,
        &context,
    )?;
    write_template(
        &component_dir.join("wit/world.wit"),
        TEMPLATE_WORLD,
        &context,
    )?;

    vendor_wit_packages(&component_dir, &context.versions)?;

    println!(
        "Component `{}` scaffolded successfully.",
        context.component_name
    );

    Ok(())
}

pub fn validate_command(args: ValidateArgs) -> Result<()> {
    let report = validate_component(&args.path, !args.skip_build)?;
    print_validation_summary(&report);
    Ok(())
}

pub fn pack_command(args: PackArgs) -> Result<()> {
    let report = validate_component(&args.path, !args.skip_build)?;
    let base_out = match args.out_dir {
        Some(ref dir) if dir.is_absolute() => dir.clone(),
        Some(ref dir) => report.component_dir.join(dir),
        None => report.component_dir.join("packs"),
    };
    fs::create_dir_all(&base_out)
        .with_context(|| format!("failed to create {}", base_out.display()))?;

    let dest_dir = base_out
        .join(&report.provider.name)
        .join(&report.provider.version);
    if dest_dir.exists() {
        fs::remove_dir_all(&dest_dir)
            .with_context(|| format!("failed to clear {}", dest_dir.display()))?;
    }
    fs::create_dir_all(&dest_dir)
        .with_context(|| format!("failed to create {}", dest_dir.display()))?;

    let artifact_file = format!("{}-{}.wasm", report.provider.name, report.provider.version);
    let dest_wasm = dest_dir.join(&artifact_file);
    fs::copy(&report.artifact_path, &dest_wasm).with_context(|| {
        format!(
            "failed to copy {} to {}",
            report.artifact_path.display(),
            dest_wasm.display()
        )
    })?;

    let mut meta = report.provider.clone();
    meta.artifact.path = artifact_file.clone();
    meta.artifact.sha256 = report.sha256.clone();
    meta.artifact.created = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .context("unable to format timestamp")?;
    meta.abi.wit_packages = report.packages.clone();

    let meta_path = dest_dir.join("meta.json");
    let meta_file = fs::File::create(&meta_path)
        .with_context(|| format!("failed to create {}", meta_path.display()))?;
    serde_json::to_writer_pretty(meta_file, &meta)
        .with_context(|| format!("failed to write {}", meta_path.display()))?;

    let mut sums =
        fs::File::create(dest_dir.join("SHA256SUMS")).context("failed to create SHA256SUMS")?;
    writeln!(sums, "{}  {}", report.sha256, artifact_file).context("failed to write SHA256SUMS")?;

    println!("✓ Packed component at {}", dest_dir.display());
    Ok(())
}

pub fn demo_run_command(args: DemoRunArgs) -> Result<()> {
    let report = validate_component(&args.path, !args.skip_build)?;
    let artifact_path = match args.artifact {
        Some(ref path) => resolve_path(&report.component_dir, path),
        None => report.artifact_path.clone(),
    };

    let cache_root = report.component_dir.join("target/demo-cache");
    let store = ComponentStore::new(&cache_root)
        .with_context(|| format!("failed to initialise cache at {}", cache_root.display()))?;
    let policy = LoadPolicy::new(Arc::new(store)).with_host_policy(HostPolicy {
        allow_http_fetch: false,
        allow_telemetry: true,
    });
    let cref = ComponentRef {
        name: report.provider.name.clone(),
        locator: artifact_path
            .canonicalize()
            .unwrap_or(artifact_path.clone())
            .display()
            .to_string(),
    };
    let handle =
        component_runtime::load(&cref, &policy).context("failed to load component into runtime")?;
    let manifest = component_runtime::describe(&handle).context("failed to describe component")?;

    let operation = args
        .operation
        .clone()
        .unwrap_or_else(|| "invoke".to_string());
    let available_ops: BTreeSet<_> = manifest
        .exports
        .iter()
        .map(|export| export.operation.clone())
        .collect();
    if !available_ops.contains(&operation) {
        bail!(
            "component does not export required operation `{}`. Available: {}",
            operation,
            available_ops.iter().cloned().collect::<Vec<_>>().join(", ")
        );
    }

    let input_value: JsonValue = if let Some(ref input) = args.input {
        serde_json::from_str(input).context("failed to parse --input JSON")?
    } else {
        json!({})
    };

    let config_value: JsonValue = if let Some(ref cfg) = args.config {
        let cfg_path = resolve_path(&report.component_dir, cfg);
        let contents = fs::read_to_string(&cfg_path)
            .with_context(|| format!("failed to read config {}", cfg_path.display()))?;
        serde_json::from_str(&contents)
            .with_context(|| format!("invalid JSON in {}", cfg_path.display()))?
    } else {
        json!({})
    };

    let mut missing_secrets = Vec::new();
    let mut provided_secrets = Vec::new();
    for secret in &manifest.secrets {
        if env::var(secret).is_ok() {
            provided_secrets.push(secret.clone());
        } else {
            missing_secrets.push(secret.clone());
        }
    }
    if !missing_secrets.is_empty() {
        println!(
            "warning: secrets not provided via environment variables: {}",
            missing_secrets.join(", ")
        );
    }

    let bindings = Bindings::new(config_value.clone(), provided_secrets);
    let env = EnvId::new("dev").context("invalid default environment id")?;
    let tenant_id = TenantId::new("demo").context("invalid default tenant id")?;
    let tenant = FlowTenantCtx::new(env, tenant_id);
    let runtime_tenant = flow_ctx_to_runtime_ctx(&tenant)?;

    let mut secret_resolver =
        |key: &str, _ctx: &RuntimeTenantCtx| -> Result<String, component_runtime::CompError> {
            match env::var(key) {
                Ok(value) => Ok(value),
                Err(_) => Err(component_runtime::CompError::Runtime(format!(
                    "secret `{}` not provided; set environment variable `{}`",
                    key, key
                ))),
            }
        };

    component_runtime::bind(&handle, &runtime_tenant, &bindings, &mut secret_resolver)
        .context("failed to bind component configuration")?;
    let output = component_runtime::invoke(&handle, &operation, &input_value, &runtime_tenant)
        .context("component invocation failed")?;

    println!(
        "{}",
        serde_json::to_string_pretty(&output)
            .context("failed to format invocation result as JSON")?
    );

    Ok(())
}

fn flow_ctx_to_runtime_ctx(ctx: &FlowTenantCtx) -> Result<RuntimeTenantCtx> {
    let serialized = serde_json::to_value(ctx)
        .context("failed to serialize tenant context for runtime mapping")?;
    serde_json::from_value(serialized)
        .context("failed to convert tenant context to runtime-compatible version")
}

fn create_dir(path: PathBuf) -> Result<()> {
    fs::create_dir_all(&path)
        .with_context(|| format!("failed to create directory `{}`", path.display()))
}

fn write_template(path: &Path, template: &str, context: &TemplateContext) -> Result<()> {
    if path.exists() {
        bail!("file `{}` already exists", path.display());
    }

    let rendered = render_template(template, context);
    fs::write(path, rendered).with_context(|| format!("failed to write `{}`", path.display()))
}

fn render_template(template: &str, context: &TemplateContext) -> String {
    let mut output = template.to_owned();
    for (key, value) in &context.placeholders {
        let token = format!("{{{{{key}}}}}");
        output = output.replace(&token, value);
    }
    output
}

fn vendor_wit_packages(component_dir: &Path, versions: &Versions) -> Result<()> {
    let deps_dir = component_dir.join("wit/deps");
    create_dir(deps_dir.clone())?;

    for info in [
        &versions.component_wit,
        &versions.host_import_wit,
        &versions.types_core_wit,
    ] {
        let package_name = info
            .dir
            .file_name()
            .ok_or_else(|| anyhow!("invalid wit directory {}", info.dir.display()))?
            .to_string_lossy()
            .replace('@', "-");
        let namespace = info
            .dir
            .parent()
            .and_then(|path| path.file_name())
            .ok_or_else(|| anyhow!("invalid wit namespace for {}", info.dir.display()))?
            .to_string_lossy()
            .into_owned();
        let dest = deps_dir.join(format!("{}-{}", namespace, package_name));
        copy_dir_recursive(&info.dir, &dest)?;
    }

    Ok(())
}

fn detect_wit_package(crate_root: &Path, prefix: &str) -> Result<WitInfo> {
    let wit_dir = crate_root.join("wit");
    let namespace_dir = wit_dir.join("greentic");
    let prefix = format!("{prefix}@");

    let mut best: Option<(Version, PathBuf)> = None;
    for entry in fs::read_dir(&namespace_dir).with_context(|| {
        format!(
            "failed to read namespace directory {}",
            namespace_dir.display()
        )
    })? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow!("non-unicode filename under {}", namespace_dir.display()))?;
        if let Some(rest) = name.strip_prefix(&prefix) {
            let version = Version::parse(rest)
                .with_context(|| format!("invalid semver `{}` for {}", rest, prefix))?;
            if best.as_ref().is_none_or(|(current, _)| &version > current) {
                best = Some((version, path));
            }
        }
    }

    match best {
        Some((version, dir)) => Ok(WitInfo {
            version: version.to_string(),
            dir,
        }),
        None => Err(anyhow!(
            "unable to locate WIT package `{}` under {}",
            prefix,
            namespace_dir.display()
        )),
    }
}

#[derive(Deserialize)]
struct LockPackage {
    name: String,
    version: String,
}

#[derive(Deserialize)]
struct LockFile {
    package: Vec<LockPackage>,
}

fn resolved_version(crate_name: &str) -> Result<String> {
    let lock_path = WORKSPACE_ROOT.join("Cargo.lock");
    let contents = fs::read_to_string(&lock_path)
        .with_context(|| format!("failed to read {}", lock_path.display()))?;
    let lock: LockFile =
        toml::from_str(&contents).with_context(|| format!("invalid {}", lock_path.display()))?;

    let mut best: Option<(Version, String)> = None;
    for pkg in lock
        .package
        .into_iter()
        .filter(|pkg| pkg.name == crate_name)
    {
        let version = Version::parse(&pkg.version)
            .with_context(|| format!("invalid semver `{}` for {}", pkg.version, crate_name))?;
        if best.as_ref().is_none_or(|(current, _)| &version > current) {
            best = Some((version, pkg.version));
        }
    }

    match best {
        Some((_, version)) => Ok(version),
        None => Err(anyhow!(
            "crate `{}` not found in {}",
            crate_name,
            lock_path.display()
        )),
    }
}

fn cargo_home() -> Result<PathBuf> {
    if let Ok(path) = env::var("CARGO_HOME") {
        return Ok(PathBuf::from(path));
    }
    if let Ok(home) = env::var("HOME") {
        return Ok(PathBuf::from(home).join(".cargo"));
    }
    Err(anyhow!(
        "unable to determine CARGO_HOME; set the environment variable explicitly"
    ))
}

fn find_crate_source(crate_name: &str, version: &str) -> Result<PathBuf> {
    let home = cargo_home()?;
    let registry_src = home.join("registry/src");
    if !registry_src.exists() {
        return Err(anyhow!(
            "cargo registry src directory not found at {}",
            registry_src.display()
        ));
    }

    for index in fs::read_dir(&registry_src)? {
        let index_path = index?.path();
        if !index_path.is_dir() {
            continue;
        }
        let candidate = index_path.join(format!("{}-{}", crate_name, version));
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(anyhow!(
        "crate `{}` version `{}` not found under {}",
        crate_name,
        version,
        registry_src.display()
    ))
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    if dest.exists() {
        fs::remove_dir_all(dest).with_context(|| format!("failed to remove {}", dest.display()))?;
    }
    fs::create_dir_all(dest).with_context(|| format!("failed to create {}", dest.display()))?;
    for entry in
        fs::read_dir(src).with_context(|| format!("failed to read directory {}", src.display()))?
    {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            fs::copy(&src_path, &dest_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    src_path.display(),
                    dest_path.display()
                )
            })?;
        }
    }
    Ok(())
}

struct TemplateContext {
    component_name: String,
    component_kebab: String,
    versions: Versions,
    placeholders: HashMap<String, String>,
}

impl TemplateContext {
    fn new(raw: &str) -> Result<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            bail!("component name cannot be empty");
        }

        let component_kebab = trimmed.to_case(Case::Kebab);
        let component_snake = trimmed.to_case(Case::Snake);
        let component_pascal = trimmed.to_case(Case::Pascal);
        let component_name = component_kebab.clone();
        let versions = VERSIONS.clone();

        let mut placeholders = HashMap::new();
        placeholders.insert("component_name".into(), component_name.clone());
        placeholders.insert("component_kebab".into(), component_kebab.clone());
        placeholders.insert("component_snake".into(), component_snake.clone());
        placeholders.insert("component_pascal".into(), component_pascal.clone());
        placeholders.insert("component_crate".into(), component_kebab.clone());
        placeholders.insert(
            "component_dir".into(),
            format!("component-{}", component_kebab),
        );
        placeholders.insert("interfaces_version".into(), versions.interfaces.clone());
        placeholders.insert("types_version".into(), versions.types.clone());
        placeholders.insert(
            "component_runtime_version".into(),
            versions.component_runtime.clone(),
        );
        placeholders.insert(
            "component_world_version".into(),
            versions.component_wit.version.clone(),
        );
        placeholders.insert(
            "host_import_version".into(),
            versions.host_import_wit.version.clone(),
        );
        placeholders.insert(
            "types_core_version".into(),
            versions.types_core_wit.version.clone(),
        );

        Ok(Self {
            component_name,
            component_kebab,
            versions,
            placeholders,
        })
    }

    fn component_dir(&self) -> String {
        format!("component-{}", self.component_kebab)
    }
}

fn print_validation_summary(report: &ValidationReport) {
    println!(
        "✓ Validated {} {}",
        report.provider.name, report.provider.version
    );
    println!("  artifact: {}", report.artifact_path.display());
    println!("  sha256 : {}", report.sha256);
    println!("  world  : {}", report.world);
    println!("  packages:");
    for pkg in &report.packages {
        println!("    - {pkg}");
    }
    if let Some(manifest) = &report.manifest {
        println!("  exports:");
        for export in &manifest.exports {
            println!("    - {}", export.operation);
        }
    } else {
        println!("  exports: <skipped - missing WASI host support>");
    }
}

fn validate_component(path: &Path, build: bool) -> Result<ValidationReport> {
    let component_dir = resolve_component_dir(path)?;

    if build {
        ensure_cargo_component_installed()?;
        run_cargo_component_build(&component_dir)?;
    }

    let provider_path = component_dir.join("provider.toml");
    let provider = load_provider(&provider_path)?;

    let versions = Versions::load()?;
    ensure_version_alignment(&provider, &versions)?;

    let mut attempted = Vec::new();
    let mut artifact_path = None;
    for candidate in candidate_artifact_paths(&provider.artifact.path) {
        let resolved = resolve_path(&component_dir, Path::new(&candidate));
        attempted.push(resolved.clone());
        if resolved.exists() {
            artifact_path = Some(resolved);
            break;
        }
    }
    let artifact_path = match artifact_path {
        Some(path) => path,
        None => {
            let paths = attempted
                .into_iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            bail!("artifact path not found; checked {}", paths);
        }
    };

    let wasm_bytes = fs::read(&artifact_path)
        .with_context(|| format!("failed to read {}", artifact_path.display()))?;
    let sha256 = format!("{:x}", Sha256::digest(&wasm_bytes));

    let decoded = decode_component(&wasm_bytes).context("failed to decode component")?;
    let (resolve, world_id) = match decoded {
        DecodedWasm::Component(resolve, world) => (resolve, world),
        DecodedWasm::WitPackage(_, _) => {
            bail!("expected a component artifact but found a WIT package bundle")
        }
    };
    let (packages, world, export_package) = extract_wit_metadata(&resolve, world_id)?;

    if packages.is_empty() {
        bail!("no WIT packages embedded in component artifact");
    }

    if provider.abi.world != world {
        if let Some(expected_pkg) = world_to_package_id(&provider.abi.world) {
            if let Some(actual_pkg) = export_package {
                if actual_pkg != expected_pkg {
                    bail!(
                        "provider world `{}` expects package '{}', but embedded exports use '{}'",
                        provider.abi.world,
                        expected_pkg,
                        actual_pkg
                    );
                }
            } else if !packages.iter().any(|pkg| pkg == &expected_pkg) {
                bail!(
                    "provider world `{}` expects package '{}', which was not embedded (found {:?})",
                    provider.abi.world,
                    expected_pkg,
                    packages
                );
            }
        } else {
            bail!(
                "provider world `{}` is not formatted as <namespace>:<package>/<world>@<version>",
                provider.abi.world
            );
        }
    }

    let expected_packages: BTreeSet<_> = provider.abi.wit_packages.iter().cloned().collect();
    if !expected_packages.is_empty() {
        let actual_greentic: BTreeSet<_> = packages
            .iter()
            .filter(|pkg| pkg.starts_with("greentic:"))
            .cloned()
            .collect();
        if !expected_packages.is_subset(&actual_greentic) {
            bail!(
                "provider wit_packages {:?} not satisfied by embedded packages {:?}",
                expected_packages,
                actual_greentic
            );
        }
    }

    let cache_root = component_dir.join("target/component-cache");
    let store = ComponentStore::new(&cache_root)
        .with_context(|| format!("failed to initialise cache at {}", cache_root.display()))?;
    let policy = LoadPolicy::new(Arc::new(store)).with_host_policy(HostPolicy {
        allow_http_fetch: false,
        allow_telemetry: true,
    });
    let cref = ComponentRef {
        name: provider.name.clone(),
        locator: artifact_path
            .canonicalize()
            .unwrap_or(artifact_path.clone())
            .display()
            .to_string(),
    };
    let manifest = match component_runtime::load(&cref, &policy) {
        Ok(handle) => {
            let manifest = component_runtime::describe(&handle)
                .context("failed to inspect component manifest")?;
            validate_exports(&provider, &manifest)?;
            validate_capabilities(&provider, &manifest)?;
            Some(manifest)
        }
        Err(component_runtime::CompError::Wasmtime(wasmtime_err)) => {
            let msg = wasmtime_err.to_string();
            if msg.contains("wasi:") {
                println!(
                    "warning: skipping runtime manifest validation due to missing WASI host support: {}",
                    msg
                );
                None
            } else {
                return Err(component_runtime::CompError::Wasmtime(wasmtime_err).into());
            }
        }
        Err(other) => return Err(other.into()),
    };

    Ok(ValidationReport {
        provider,
        component_dir,
        artifact_path,
        sha256,
        world,
        packages,
        manifest,
    })
}

fn resolve_component_dir(path: &Path) -> Result<PathBuf> {
    let dir = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .context("unable to determine current directory")?
            .join(path)
    };
    dir.canonicalize()
        .with_context(|| format!("failed to canonicalize {}", dir.display()))
}

fn resolve_path(base: &Path, raw: impl AsRef<Path>) -> PathBuf {
    let raw_path = raw.as_ref();
    if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        base.join(raw_path)
    }
}

fn candidate_artifact_paths(original: &str) -> Vec<String> {
    let mut paths = Vec::new();
    paths.push(original.to_string());

    for (from, to) in [
        ("wasm32-wasip2", "wasm32-wasip1"),
        ("wasm32-wasip2", "wasm32-wasi"),
        ("wasm32-wasip1", "wasm32-wasip2"),
        ("wasm32-wasip1", "wasm32-wasi"),
        ("wasm32-wasi", "wasm32-wasip2"),
        ("wasm32-wasi", "wasm32-wasip1"),
    ] {
        if original.contains(from) {
            let candidate = original.replace(from, to);
            if candidate != original && !paths.contains(&candidate) {
                paths.push(candidate);
            }
        }
    }

    paths
}

fn ensure_cargo_component_installed() -> Result<()> {
    let status = Command::new("cargo")
        .arg("component")
        .arg("--version")
        .status();
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => bail!(
            "cargo-component is required. Install with `cargo install cargo-component --locked`."
        ),
        Err(err) => Err(anyhow!(
            "failed to execute `cargo component --version`: {err}. Install cargo-component with `cargo install cargo-component --locked`."
        )),
    }
}

fn run_cargo_component_build(component_dir: &Path) -> Result<()> {
    let cache_dir = component_dir.join("target").join(".component-cache");
    let status = Command::new("cargo")
        .current_dir(component_dir)
        .arg("component")
        .arg("build")
        .arg("--release")
        .arg("--target")
        .arg("wasm32-wasip2")
        .env("CARGO_COMPONENT_CACHE_DIR", cache_dir.as_os_str())
        .env("CARGO_NET_OFFLINE", "true")
        .status()
        .with_context(|| {
            format!(
                "failed to run `cargo component build` in {}",
                component_dir.display()
            )
        })?;
    if status.success() {
        Ok(())
    } else {
        bail!("cargo component build failed")
    }
}

fn load_provider(path: &Path) -> Result<ProviderMetadata> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read provider metadata {}", path.display()))?;
    let provider: ProviderMetadata =
        toml::from_str(&contents).context("provider.toml is not valid TOML")?;
    if provider.artifact.format != "wasm-component" {
        bail!(
            "artifact.format must be `wasm-component`, found `{}`",
            provider.artifact.format
        );
    }
    Ok(provider)
}

fn ensure_version_alignment(provider: &ProviderMetadata, versions: &Versions) -> Result<()> {
    if provider.abi.interfaces_version != versions.interfaces {
        bail!(
            "provider abi.interfaces_version `{}` does not match pinned `{}`",
            provider.abi.interfaces_version,
            versions.interfaces
        );
    }
    if provider.abi.types_version != versions.types {
        bail!(
            "provider abi.types_version `{}` does not match pinned `{}`",
            provider.abi.types_version,
            versions.types
        );
    }
    if provider.abi.component_runtime != versions.component_runtime {
        bail!(
            "provider abi.component_runtime `{}` does not match pinned `{}`",
            provider.abi.component_runtime,
            versions.component_runtime
        );
    }
    Ok(())
}

fn extract_wit_metadata(
    resolve: &Resolve,
    world_id: WorldId,
) -> Result<(Vec<String>, String, Option<String>)> {
    let mut packages = Vec::new();
    for (_, package) in resolve.packages.iter() {
        let name = &package.name;
        if name.namespace == "root" {
            continue;
        }
        if let Some(version) = &name.version {
            packages.push(format!("{}:{}@{}", name.namespace, name.name, version));
        } else {
            packages.push(format!("{}:{}", name.namespace, name.name));
        }
    }
    packages.sort();
    packages.dedup();

    let world = &resolve.worlds[world_id];
    let mut export_package = None;
    for item in world.exports.values() {
        if let WorldItem::Interface { id, .. } = item {
            let iface = &resolve.interfaces[*id];
            if let Some(pkg_id) = iface.package {
                let pkg = &resolve.packages[pkg_id].name;
                if pkg.namespace != "root" {
                    let mut ident = format!("{}:{}", pkg.namespace, pkg.name);
                    if let Some(version) = &pkg.version {
                        ident.push('@');
                        ident.push_str(&version.to_string());
                    }
                    export_package.get_or_insert(ident);
                }
            }
        }
    }

    let world_string = if let Some(pkg_id) = world.package {
        let pkg = &resolve.packages[pkg_id];
        if let Some(version) = &pkg.name.version {
            format!(
                "{}:{}/{}@{}",
                pkg.name.namespace, pkg.name.name, world.name, version
            )
        } else {
            format!("{}:{}/{}", pkg.name.namespace, pkg.name.name, world.name)
        }
    } else {
        world.name.clone()
    };

    Ok((packages, world_string, export_package))
}

fn world_to_package_id(world: &str) -> Option<String> {
    let (pkg_part, rest) = world.split_once('/')?;
    let (_, version) = rest.rsplit_once('@')?;
    Some(format!("{}@{}", pkg_part, version))
}

fn validate_exports(provider: &ProviderMetadata, manifest: &ComponentManifestInfo) -> Result<()> {
    let actual: BTreeSet<_> = manifest
        .exports
        .iter()
        .map(|export| export.operation.clone())
        .collect();
    for required in &provider.exports.provides {
        if !actual.contains(required) {
            bail!(
                "component manifest is missing required export `{}`",
                required
            );
        }
    }
    Ok(())
}

fn validate_capabilities(
    provider: &ProviderMetadata,
    manifest: &ComponentManifestInfo,
) -> Result<()> {
    let actual: BTreeSet<_> = manifest
        .capabilities
        .iter()
        .map(|cap| cap.as_str().to_string())
        .collect();
    for (name, required) in [
        ("secrets", provider.capabilities.secrets),
        ("telemetry", provider.capabilities.telemetry),
        ("network", provider.capabilities.network),
        ("filesystem", provider.capabilities.filesystem),
    ] {
        if required && !actual.contains(name) {
            bail!(
                "provider declares capability `{}` but component manifest does not expose it",
                name
            );
        }
    }
    Ok(())
}
