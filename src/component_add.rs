use std::convert::TryFrom;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result, anyhow, bail};
use bytes::Bytes;
use greentic_distributor_client::{
    DistributorClient, DistributorClientConfig, DistributorEnvironmentId, HttpDistributorClient,
    ResolveComponentRequest,
};
use greentic_pack::builder::ComponentEntry;
use greentic_types::{EnvId, TenantCtx, TenantId};
use reqwest::blocking::Client;
use semver::Version;
use serde_json::json;
use tokio::runtime::Runtime;

use crate::config;
use crate::distributor;
use crate::pack_init::{
    PackInitIntent, WorkspaceComponent, WorkspaceManifest, manifest_path, slugify,
};

pub fn run_component_add(
    coordinate: &str,
    profile: Option<&str>,
    intent: PackInitIntent,
) -> Result<PathBuf> {
    let cfg = config::load_with_meta(None)?;
    let profile = distributor::resolve_profile(&cfg, profile)?;
    let (component_id, version_req) = parse_coordinate(coordinate)?;
    let tenant_ctx = build_tenant_ctx(&profile)?;
    let environment_id = DistributorEnvironmentId::from(profile.environment_id.as_str());
    let pack_id = detect_pack_id().unwrap_or_else(|| "greentic-dev-local".to_string());

    let req = ResolveComponentRequest {
        tenant: tenant_ctx.clone(),
        environment_id,
        pack_id,
        component_id: component_id.clone(),
        version: version_req.to_string(),
        extra: json!({ "intent": format!("{:?}", intent) }),
    };

    let client = http_client(&profile)?;
    let rt = Runtime::new().context("failed to start tokio runtime for distributor client")?;
    let response = rt.block_on(client.resolve_component(req))?;

    let artifact_bytes = fetch_artifact(&response.artifact)?;
    let (cache_dir, cache_path) =
        write_component_to_cache(&component_id, &version_req, &artifact_bytes)?;
    update_manifest(coordinate, &component_id, &version_req, &cache_path)?;

    println!(
        "Resolved {} -> {}@{}",
        coordinate, component_id, version_req
    );
    println!("Cached component at {}", cache_path.display());
    println!(
        "Updated workspace manifest at {}",
        manifest_path()?.display()
    );

    Ok(cache_dir)
}

fn parse_coordinate(input: &str) -> Result<(String, String)> {
    if let Some((id, ver)) = input.rsplit_once('@') {
        Ok((id.to_string(), ver.to_string()))
    } else {
        Ok((input.to_string(), "*".to_string()))
    }
}

fn build_tenant_ctx(profile: &distributor::DistributorProfile) -> Result<TenantCtx> {
    let env = EnvId::from_str(&profile.environment_id)
        .or_else(|_| EnvId::try_from(profile.environment_id.as_str()))
        .map_err(|err| anyhow!("invalid environment id `{}`: {err}", profile.environment_id))?;
    let tenant = TenantId::from_str(&profile.tenant_id)
        .or_else(|_| TenantId::try_from(profile.tenant_id.as_str()))
        .map_err(|err| anyhow!("invalid tenant id `{}`: {err}", profile.tenant_id))?;
    Ok(TenantCtx::new(env, tenant))
}

fn http_client(profile: &distributor::DistributorProfile) -> Result<HttpDistributorClient> {
    let env_id = EnvId::from_str(&profile.environment_id)
        .or_else(|_| EnvId::try_from(profile.environment_id.as_str()))
        .map_err(|err| anyhow!("invalid environment id `{}`: {err}", profile.environment_id))?;
    let tenant_id = TenantId::from_str(&profile.tenant_id)
        .or_else(|_| TenantId::try_from(profile.tenant_id.as_str()))
        .map_err(|err| anyhow!("invalid tenant id `{}`: {err}", profile.tenant_id))?;
    let cfg = DistributorClientConfig {
        base_url: Some(profile.url.clone()),
        environment_id: DistributorEnvironmentId::from(profile.environment_id.as_str()),
        tenant: TenantCtx::new(env_id, tenant_id),
        auth_token: profile.token.clone(),
        extra_headers: profile.headers.clone(),
        request_timeout: None,
    };
    HttpDistributorClient::new(cfg).map_err(Into::into)
}

fn fetch_artifact(location: &greentic_distributor_client::ArtifactLocation) -> Result<Bytes> {
    match location {
        greentic_distributor_client::ArtifactLocation::FilePath { path } => {
            if path.starts_with("http://") || path.starts_with("https://") {
                let client = Client::new();
                let resp = client
                    .get(path)
                    .send()
                    .context("failed to download artifact")?;
                if !resp.status().is_success() {
                    bail!("artifact download failed with status {}", resp.status());
                }
                resp.bytes().map_err(Into::into)
            } else if let Some(rest) = path.strip_prefix("file://") {
                fs::read(rest)
                    .map(Bytes::from)
                    .with_context(|| format!("failed to read component at {}", rest))
            } else {
                fs::read(path)
                    .map(Bytes::from)
                    .with_context(|| format!("failed to read component at {}", path))
            }
        }
        greentic_distributor_client::ArtifactLocation::OciReference { reference } => {
            bail!("OCI component artifacts are not supported yet ({reference})")
        }
        greentic_distributor_client::ArtifactLocation::DistributorInternal { handle } => {
            bail!("Distributor internal artifacts are not supported yet ({handle})")
        }
    }
}

fn write_component_to_cache(
    component_id: &str,
    version: &str,
    bytes: &Bytes,
) -> Result<(PathBuf, PathBuf)> {
    let mut path = cache_base_dir()?;
    let slug = cache_slug_parts(component_id, version);
    path.push(slug);
    fs::create_dir_all(&path).with_context(|| format!("failed to create {}", path.display()))?;
    let file_path = path.join("artifact.wasm");
    fs::write(&file_path, bytes)
        .with_context(|| format!("failed to write {}", file_path.display()))?;
    Ok((path, file_path))
}

fn cache_base_dir() -> Result<PathBuf> {
    let mut base = std::env::current_dir().context("unable to determine workspace root")?;
    base.push(".greentic");
    base.push("components");
    fs::create_dir_all(&base)
        .with_context(|| format!("failed to create cache directory {}", base.display()))?;
    Ok(base)
}

fn cache_slug_parts(component_id: &str, version: &str) -> String {
    slugify(&format!("{}-{}", component_id.replace('/', "-"), version))
}

fn update_manifest(
    coordinate: &str,
    component_id: &str,
    version: &str,
    wasm_path: &Path,
) -> Result<()> {
    let manifest_path = manifest_path()?;
    let mut manifest: WorkspaceManifest = if manifest_path.exists() {
        let data = fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        serde_json::from_str(&data)
            .with_context(|| format!("failed to parse {}", manifest_path.display()))?
    } else {
        WorkspaceManifest::default()
    };

    let entry = WorkspaceComponent {
        coordinate: coordinate.to_string(),
        entry: ComponentEntry {
            name: component_id.to_string(),
            version: Version::parse(version).unwrap_or_else(|_| Version::new(0, 0, 0)),
            file_wasm: wasm_path.display().to_string(),
            hash_blake3: String::new(),
            schema_file: None,
            manifest_file: None,
            world: None,
            capabilities: None,
        },
    };

    let mut replaced = false;
    for existing in manifest.components.iter_mut() {
        if existing.entry.name == entry.entry.name {
            *existing = entry.clone();
            replaced = true;
            break;
        }
    }
    if !replaced {
        manifest.components.push(entry);
    }

    let rendered =
        serde_json::to_string_pretty(&manifest).context("failed to render workspace manifest")?;
    fs::write(&manifest_path, rendered)
        .with_context(|| format!("failed to write {}", manifest_path.display()))?;
    Ok(())
}

fn detect_pack_id() -> Option<String> {
    let candidates = ["pack.toml", "Pack.toml"];
    for candidate in candidates {
        let path = Path::new(candidate);
        if path.exists() {
            let data = fs::read_to_string(path).ok()?;
            if let Ok(value) = data.parse::<toml::Value>()
                && let Some(id) = value.get("pack_id").and_then(|v| v.as_str())
            {
                return Some(id.to_string());
            }
        }
    }
    None
}
