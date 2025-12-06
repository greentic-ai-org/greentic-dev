use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use bytes::Bytes;
use greentic_pack::builder::ComponentEntry;
use semver::Version;
use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use crate::config;
use crate::distributor::{
    DevArtifactKind, DevDistributorClient, DevDistributorError, DevIntent, DevResolveRequest,
    DevResolveResponse, resolve_profile,
};

#[derive(Debug, Clone, Copy)]
pub enum PackInitIntent {
    Dev,
    Runtime,
}

pub fn run(from: &str, profile: Option<&str>) -> Result<()> {
    let config = config::load()?;
    let profile = resolve_profile(&config, profile)?;
    let client = DevDistributorClient::from_profile(profile.clone())?;

    let resolve = client.resolve(&DevResolveRequest {
        coordinate: from.to_string(),
        intent: DevIntent::Dev,
        platform: Some(default_platform()),
        features: Vec::new(),
    });

    let resolved = handle_resolve_result(resolve)?;
    if resolved.kind != DevArtifactKind::Pack {
        bail!(
            "coordinate `{}` resolved to {:?}, expected pack",
            resolved.coordinate,
            resolved.kind
        );
    }

    let bytes = client.download_artifact(&resolved.artifact_download_path)?;
    let cache_path = write_pack_to_cache(&resolved, &bytes)?;
    let workspace_dir = slug_to_dir(&resolved.name)?;
    fs::create_dir(&workspace_dir).with_context(|| {
        format!(
            "failed to create workspace directory {}",
            workspace_dir.display()
        )
    })?;

    let bundle_path = workspace_dir.join("bundle.gtpack");
    fs::write(&bundle_path, &bytes)
        .with_context(|| format!("failed to write {}", bundle_path.display()))?;
    unpack_gtpack(&workspace_dir, bytes.clone())?;

    println!(
        "Initialized pack {}@{} in {} (cached at {})",
        resolved.name,
        resolved.version,
        workspace_dir.display(),
        cache_path.display()
    );

    Ok(())
}

pub fn run_component_add(
    coordinate: &str,
    profile: Option<&str>,
    intent: PackInitIntent,
) -> Result<()> {
    let config = config::load()?;
    let profile = resolve_profile(&config, profile)?;
    let client = DevDistributorClient::from_profile(profile.clone())?;

    let resolve = client.resolve(&DevResolveRequest {
        coordinate: coordinate.to_string(),
        intent: match intent {
            PackInitIntent::Dev => DevIntent::Dev,
            PackInitIntent::Runtime => DevIntent::Runtime,
        },
        platform: Some(default_platform()),
        features: Vec::new(),
    });
    let resolved = handle_resolve_result(resolve)?;
    if resolved.kind != DevArtifactKind::Component {
        bail!(
            "coordinate `{}` resolved to {:?}, expected component",
            resolved.coordinate,
            resolved.kind
        );
    }

    let bytes = client.download_artifact(&resolved.artifact_download_path)?;
    let cache_path = write_component_to_cache(&resolved, &bytes)?;
    update_workspace_manifest(&resolved, &cache_path)?;

    println!(
        "Resolved {} -> {}@{}",
        resolved.coordinate, resolved.name, resolved.version
    );
    println!("Cached component at {}", cache_path.display());
    println!(
        "Updated workspace manifest at {}",
        manifest_path()?.display()
    );
    Ok(())
}

fn default_platform() -> String {
    "wasm32-wasip2".to_string()
}

fn handle_resolve_result(
    result: Result<DevResolveResponse, DevDistributorError>,
) -> Result<DevResolveResponse> {
    match result {
        Ok(resp) => Ok(resp),
        Err(DevDistributorError::LicenseRequired(body)) => bail!(
            "license required for {}: {}\nCheckout URL: {}",
            body.coordinate,
            body.message,
            body.checkout_url
        ),
        Err(other) => Err(anyhow!(other)),
    }
}

fn cache_base_dir() -> Result<PathBuf> {
    let mut base = dirs::home_dir().ok_or_else(|| anyhow!("unable to determine home directory"))?;
    base.push(".greentic");
    base.push("cache");
    fs::create_dir_all(&base)
        .with_context(|| format!("failed to create cache directory {}", base.display()))?;
    Ok(base)
}

fn write_component_to_cache(resolved: &DevResolveResponse, bytes: &Bytes) -> Result<PathBuf> {
    let mut path = cache_base_dir()?;
    path.push("components");
    let slug = cache_slug(resolved);
    path.push(slug);
    fs::create_dir_all(&path).with_context(|| format!("failed to create {}", path.display()))?;
    let file_path = path.join("artifact.wasm");
    fs::write(&file_path, bytes)
        .with_context(|| format!("failed to write {}", file_path.display()))?;
    Ok(file_path)
}

fn write_pack_to_cache(resolved: &DevResolveResponse, bytes: &Bytes) -> Result<PathBuf> {
    let mut path = cache_base_dir()?;
    path.push("packs");
    let slug = cache_slug(resolved);
    path.push(slug);
    fs::create_dir_all(&path).with_context(|| format!("failed to create {}", path.display()))?;
    let file_path = path.join("bundle.gtpack");
    fs::write(&file_path, bytes)
        .with_context(|| format!("failed to write {}", file_path.display()))?;
    Ok(file_path)
}

fn cache_slug(resolved: &DevResolveResponse) -> String {
    if let Some(digest) = &resolved.digest {
        return digest.replace(':', "-");
    }
    slugify(&format!("{}-{}", resolved.name, resolved.version))
}

fn slugify(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in raw.chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn manifest_path() -> Result<PathBuf> {
    let mut root = std::env::current_dir().context("unable to determine current directory")?;
    root.push(".greentic");
    fs::create_dir_all(&root).with_context(|| format!("failed to create {}", root.display()))?;
    Ok(root.join("manifest.json"))
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct WorkspaceManifest {
    components: Vec<WorkspaceComponent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkspaceComponent {
    coordinate: String,
    entry: ComponentEntry,
}

fn update_workspace_manifest(resolved: &DevResolveResponse, cache_path: &Path) -> Result<()> {
    let manifest_path = manifest_path()?;
    let mut manifest: WorkspaceManifest = if manifest_path.exists() {
        let data = fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        serde_json::from_str(&data)
            .with_context(|| format!("failed to parse {}", manifest_path.display()))?
    } else {
        WorkspaceManifest::default()
    };

    let version = Version::parse(&resolved.version)
        .with_context(|| format!("invalid semver version `{}`", resolved.version))?;
    let entry = ComponentEntry {
        name: resolved.name.clone(),
        version,
        file_wasm: cache_path.display().to_string(),
        hash_blake3: resolved.digest.clone().unwrap_or_default(),
        schema_file: None,
        manifest_file: None,
        world: None,
        capabilities: None,
    };

    let mut replaced = false;
    for existing in manifest.components.iter_mut() {
        if existing.entry.name == entry.name {
            existing.coordinate = resolved.coordinate.clone();
            existing.entry = entry.clone();
            replaced = true;
            break;
        }
    }
    if !replaced {
        manifest.components.push(WorkspaceComponent {
            coordinate: resolved.coordinate.clone(),
            entry,
        });
    }

    let rendered =
        serde_json::to_string_pretty(&manifest).context("failed to render workspace manifest")?;
    fs::write(&manifest_path, rendered)
        .with_context(|| format!("failed to write {}", manifest_path.display()))?;
    Ok(())
}

fn slug_to_dir(name: &str) -> Result<PathBuf> {
    let slug = slugify(name);
    let root = std::env::current_dir().context("unable to determine current directory")?;
    let dest = root.join(slug);
    if dest.exists() {
        bail!(
            "destination {} already exists; choose a different directory or remove it first",
            dest.display()
        );
    }
    Ok(dest)
}

fn unpack_gtpack(dest: &Path, bytes: Bytes) -> Result<()> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).context("failed to open gtpack archive")?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).context("failed to read gtpack entry")?;
        let name = match file.enclosed_name() {
            Some(path) => path.to_owned(),
            None => bail!("gtpack contained a suspicious path; aborting extract"),
        };
        let out_path = dest.join(name);
        if file.name().ends_with('/') {
            fs::create_dir_all(&out_path)
                .with_context(|| format!("failed to create {}", out_path.display()))?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)
                .context("failed to read gtpack entry")?;
            let mut out = fs::File::create(&out_path)
                .with_context(|| format!("failed to create {}", out_path.display()))?;
            out.write_all(&buffer)
                .with_context(|| format!("failed to write {}", out_path.display()))?;
        }
    }
    Ok(())
}
