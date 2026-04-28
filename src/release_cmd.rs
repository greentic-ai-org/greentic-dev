use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use oci_distribution::Reference;
use oci_distribution::client::{Client, ClientConfig, ClientProtocol, Config, ImageLayer};
use oci_distribution::secrets::RegistryAuth;
use semver::Version;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::cli::{
    ReleaseDevArgs, ReleaseGenerateArgs, ReleasePromoteArgs, ReleasePublishArgs, ReleaseViewArgs,
};
use crate::install::block_on_maybe_runtime;
use crate::toolchain_catalogue::GREENTIC_TOOLCHAIN_PACKAGES;

const DEFAULT_OAUTH_USER: &str = "oauth2";
pub const TOOLCHAIN_MANIFEST_SCHEMA: &str = "greentic.toolchain-manifest.v1";
pub const TOOLCHAIN_NAME: &str = "gtc";
pub const TOOLCHAIN_LAYER_MEDIA_TYPE: &str = "application/vnd.greentic.toolchain.manifest.v1+json";
const TOOLCHAIN_CONFIG_MEDIA_TYPE: &str = "application/vnd.greentic.toolchain.config.v1+json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolchainManifest {
    pub schema: String,
    pub toolchain: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    pub packages: Vec<ToolchainPackage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolchainPackage {
    #[serde(rename = "crate")]
    pub crate_name: String,
    pub bins: Vec<String>,
    pub version: String,
}

pub fn generate(args: ReleaseGenerateArgs) -> Result<()> {
    let resolver = CargoSearchVersionResolver;
    let source = block_on_maybe_runtime(load_source_manifest(
        &args.repo,
        &args.from,
        args.token.as_deref(),
    ))
    .with_context(|| {
        format!(
            "failed to resolve source manifest `{}`",
            toolchain_ref(&args.repo, &args.from)
        )
    })?;
    let source = match source {
        Some(source) => Some(source),
        None => bootstrap_source_manifest_if_needed(
            &args.repo,
            &args.from,
            args.token.as_deref(),
            args.dry_run,
            &resolver,
        )?,
    };
    let manifest = generate_manifest(
        &args.release,
        &args.from,
        source.as_ref(),
        &resolver,
        Some(created_at_now()?),
    )?;
    if args.dry_run {
        println!("{}", serde_json::to_string_pretty(&manifest)?);
        return Ok(());
    }
    let path = write_manifest(&args.out, &manifest)?;
    println!("Wrote {}", path.display());
    Ok(())
}

fn bootstrap_source_manifest_if_needed<R: CrateVersionResolver>(
    repo: &str,
    tag: &str,
    token: Option<&str>,
    dry_run: bool,
    resolver: &R,
) -> Result<Option<ToolchainManifest>> {
    let manifest = bootstrap_source_manifest(tag, resolver, Some(created_at_now()?))?;
    if dry_run {
        eprintln!(
            "Dry run: would bootstrap missing source manifest {}",
            toolchain_ref(repo, tag)
        );
        return Ok(Some(manifest));
    }

    let auth = match optional_registry_auth(token)? {
        RegistryAuth::Anonymous => {
            eprintln!(
                "Source manifest {} is missing; no GHCR token is available, so only the local release manifest will be generated.",
                toolchain_ref(repo, tag)
            );
            return Ok(Some(manifest));
        }
        auth => auth,
    };
    block_on_maybe_runtime(async {
        let client = oci_client();
        let source_ref = parse_reference(repo, tag)?;
        push_manifest_layer(&client, &source_ref, &auth, &manifest).await
    })
    .with_context(|| format!("failed to bootstrap {}", toolchain_ref(repo, tag)))?;
    println!("Bootstrapped {}", toolchain_ref(repo, tag));
    Ok(Some(manifest))
}

fn bootstrap_source_manifest<R: CrateVersionResolver>(
    tag: &str,
    resolver: &R,
    created_at: Option<String>,
) -> Result<ToolchainManifest> {
    generate_manifest(tag, tag, None, resolver, created_at)
}

pub fn publish(args: ReleasePublishArgs) -> Result<()> {
    let (release, manifest, source) = publish_manifest_input(&args)?;

    if args.dry_run {
        println!(
            "Dry run: would publish {}",
            toolchain_ref(&args.repo, &release)
        );
        if let Some(tag) = &args.tag {
            println!(
                "Dry run: would tag {} as {}",
                toolchain_ref(&args.repo, &release),
                toolchain_ref(&args.repo, tag)
            );
        }
        return Ok(());
    }

    let auth = registry_auth(args.token.as_deref())?;
    block_on_maybe_runtime(async {
        let client = oci_client();
        let release_ref = parse_reference(&args.repo, &release)?;
        if !args.force && manifest_exists(&client, &release_ref, &auth).await? {
            bail!(
                "release tag `{}` already exists; pass --force to overwrite it",
                toolchain_ref(&args.repo, &release)
            );
        }
        push_manifest_layer(&client, &release_ref, &auth, &manifest).await?;
        if let Some(tag) = &args.tag {
            let tag_ref = parse_reference(&args.repo, tag)?;
            push_manifest_layer(&client, &tag_ref, &auth, &manifest).await?;
        }
        Ok(())
    })?;

    if let Some(source) = source {
        match source {
            PublishManifestSource::Generated(path) => println!("Wrote {}", path.display()),
            PublishManifestSource::Local(path) => println!("Read {}", path.display()),
        }
    }
    println!("Published {}", toolchain_ref(&args.repo, &release));
    if let Some(tag) = &args.tag {
        println!("Updated {}", toolchain_ref(&args.repo, tag));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PublishManifestSource {
    Generated(PathBuf),
    Local(PathBuf),
}

fn publish_manifest_input(
    args: &ReleasePublishArgs,
) -> Result<(String, ToolchainManifest, Option<PublishManifestSource>)> {
    if let Some(path) = &args.manifest {
        let mut manifest = read_manifest_file(path)?;
        validate_manifest(&manifest)?;
        let release = if let Some(release) = &args.release {
            manifest.version = release.clone();
            release.clone()
        } else {
            manifest.version.clone()
        };
        return Ok((
            release,
            manifest,
            Some(PublishManifestSource::Local(path.clone())),
        ));
    }

    let release = args
        .release
        .as_deref()
        .context("pass --release or --manifest")?;
    let from = args.from.as_deref().unwrap_or("dev");
    let resolver = CargoSearchVersionResolver;
    let source = block_on_maybe_runtime(load_source_manifest(
        &args.repo,
        from,
        args.token.as_deref(),
    ))
    .with_context(|| {
        format!(
            "failed to resolve source manifest `{}`",
            toolchain_ref(&args.repo, from)
        )
    })?;
    let manifest = generate_manifest(
        release,
        from,
        source.as_ref(),
        &resolver,
        Some(created_at_now()?),
    )?;
    let path = if args.dry_run {
        println!("{}", serde_json::to_string_pretty(&manifest)?);
        None
    } else {
        Some(PublishManifestSource::Generated(write_manifest(
            &args.out, &manifest,
        )?))
    };
    Ok((release.to_string(), manifest, path))
}

fn read_manifest_file(path: &Path) -> Result<ToolchainManifest> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("failed to parse {}", path.display()))
}

pub fn promote(args: ReleasePromoteArgs) -> Result<()> {
    if args.dry_run {
        println!(
            "Dry run: would promote {} to {}",
            toolchain_ref(&args.repo, &args.release),
            toolchain_ref(&args.repo, &args.tag)
        );
        return Ok(());
    }

    let auth = registry_auth(args.token.as_deref())?;
    block_on_maybe_runtime(async {
        let client = oci_client();
        let source_ref = parse_reference(&args.repo, &args.release)?;
        let target_ref = parse_reference(&args.repo, &args.tag)?;
        let (manifest, _) = client
            .pull_manifest(&source_ref, &auth)
            .await
            .with_context(|| {
                format!(
                    "failed to resolve source release `{}`",
                    toolchain_ref(&args.repo, &args.release)
                )
            })?;
        client
            .push_manifest(&target_ref, &manifest)
            .await
            .with_context(|| {
                format!(
                    "failed to update tag `{}`",
                    toolchain_ref(&args.repo, &args.tag)
                )
            })?;
        Ok(())
    })?;
    println!(
        "Promoted {} to {}",
        toolchain_ref(&args.repo, &args.release),
        toolchain_ref(&args.repo, &args.tag)
    );
    Ok(())
}

pub fn view(args: ReleaseViewArgs) -> Result<()> {
    let tag = release_view_tag(&args)?;
    let manifest = block_on_maybe_runtime(load_source_manifest(
        &args.repo,
        &tag,
        args.token.as_deref(),
    ))
    .with_context(|| {
        format!(
            "failed to resolve manifest `{}`",
            toolchain_ref(&args.repo, &tag)
        )
    })?
    .with_context(|| {
        format!(
            "manifest `{}` was not found or is not authorized for this token",
            toolchain_ref(&args.repo, &tag)
        )
    })?;
    println!("{}", serde_json::to_string_pretty(&manifest)?);
    Ok(())
}

pub fn dev(args: ReleaseDevArgs) -> Result<()> {
    let manifest = latest_dev_manifest(Some(created_at_now()?));
    if args.dry_run {
        println!("{}", serde_json::to_string_pretty(&manifest)?);
        println!(
            "Dry run: would publish {}",
            toolchain_ref(&args.repo, "dev")
        );
        return Ok(());
    }

    let auth = registry_auth(args.token.as_deref())?;
    block_on_maybe_runtime(async {
        let client = oci_client();
        let dev_ref = parse_reference(&args.repo, "dev")?;
        if !args.force && manifest_exists(&client, &dev_ref, &auth).await? {
            bail!(
                "dev tag `{}` already exists; pass --force to overwrite it",
                toolchain_ref(&args.repo, "dev")
            );
        }
        push_manifest_layer(&client, &dev_ref, &auth, &manifest).await
    })?;
    println!("Published {}", toolchain_ref(&args.repo, "dev"));
    Ok(())
}

fn latest_dev_manifest(created_at: Option<String>) -> ToolchainManifest {
    ToolchainManifest {
        schema: TOOLCHAIN_MANIFEST_SCHEMA.to_string(),
        toolchain: TOOLCHAIN_NAME.to_string(),
        version: "dev".to_string(),
        channel: Some("dev".to_string()),
        created_at,
        packages: GREENTIC_TOOLCHAIN_PACKAGES
            .iter()
            .map(|package| ToolchainPackage {
                crate_name: package.crate_name.to_string(),
                bins: package.bins.iter().map(|bin| (*bin).to_string()).collect(),
                version: "latest".to_string(),
            })
            .collect(),
    }
}

fn release_view_tag(args: &ReleaseViewArgs) -> Result<String> {
    match (&args.release, &args.tag) {
        (Some(release), None) => Ok(release.clone()),
        (None, Some(tag)) => Ok(tag.clone()),
        _ => bail!("pass exactly one of --release or --tag"),
    }
}

pub fn generate_manifest<R: CrateVersionResolver>(
    release: &str,
    from: &str,
    source: Option<&ToolchainManifest>,
    resolver: &R,
    created_at: Option<String>,
) -> Result<ToolchainManifest> {
    if let Some(source) = source {
        validate_manifest(source)?;
    }
    let source_versions = source_version_map(source);
    let mut packages = Vec::new();
    for package in GREENTIC_TOOLCHAIN_PACKAGES {
        let source_version = source_versions.get(package.crate_name);
        let version = match source_version.map(String::as_str) {
            Some(version) if version != "latest" => version.to_string(),
            _ => resolver.resolve_latest(package.crate_name)?,
        };
        packages.push(ToolchainPackage {
            crate_name: package.crate_name.to_string(),
            bins: package.bins.iter().map(|bin| (*bin).to_string()).collect(),
            version,
        });
    }
    Ok(ToolchainManifest {
        schema: TOOLCHAIN_MANIFEST_SCHEMA.to_string(),
        toolchain: TOOLCHAIN_NAME.to_string(),
        version: release.to_string(),
        channel: source
            .and_then(|manifest| manifest.channel.clone())
            .or_else(|| Some(from.to_string())),
        created_at,
        packages,
    })
}

pub fn validate_manifest(manifest: &ToolchainManifest) -> Result<()> {
    if manifest.schema != TOOLCHAIN_MANIFEST_SCHEMA {
        bail!(
            "unsupported toolchain manifest schema `{}`",
            manifest.schema
        );
    }
    if manifest.toolchain != TOOLCHAIN_NAME {
        bail!("unsupported toolchain `{}`", manifest.toolchain);
    }
    Ok(())
}

pub fn toolchain_ref(repo: &str, tag: &str) -> String {
    format!("{repo}:{tag}")
}

fn source_version_map(source: Option<&ToolchainManifest>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Some(source) = source {
        for package in &source.packages {
            out.insert(package.crate_name.clone(), package.version.clone());
        }
    }
    out
}

fn write_manifest(out_dir: &Path, manifest: &ToolchainManifest) -> Result<PathBuf> {
    fs::create_dir_all(out_dir)
        .with_context(|| format!("failed to create {}", out_dir.display()))?;
    let path = out_dir.join(format!("gtc-{}.json", manifest.version));
    let json = serde_json::to_vec_pretty(manifest).context("failed to serialize manifest")?;
    fs::write(&path, json).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

fn created_at_now() -> Result<String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .context("failed to format current time")
}

pub trait CrateVersionResolver {
    fn resolve_latest(&self, crate_name: &str) -> Result<String>;
}

struct CargoSearchVersionResolver;

impl CrateVersionResolver for CargoSearchVersionResolver {
    fn resolve_latest(&self, crate_name: &str) -> Result<String> {
        let output = Command::new("cargo")
            .arg("search")
            .arg(crate_name)
            .arg("--limit")
            .arg("1")
            .output()
            .with_context(|| format!("failed to execute `cargo search {crate_name} --limit 1`"))?;
        if !output.status.success() {
            bail!(
                "`cargo search {crate_name} --limit 1` failed with exit code {:?}",
                output.status.code()
            );
        }
        let stdout = String::from_utf8(output.stdout).with_context(|| {
            format!("`cargo search {crate_name} --limit 1` returned non-UTF8 output")
        })?;
        parse_cargo_search_version(crate_name, &stdout)
    }
}

fn parse_cargo_search_version(crate_name: &str, stdout: &str) -> Result<String> {
    let first_line = stdout
        .lines()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| anyhow!("`cargo search {crate_name} --limit 1` returned no results"))?;
    let Some((found_name, rhs)) = first_line.split_once('=') else {
        bail!("unexpected cargo search output: {first_line}");
    };
    if found_name.trim() != crate_name {
        bail!(
            "`cargo search {crate_name} --limit 1` returned `{}` first",
            found_name.trim()
        );
    }
    let quoted = rhs
        .split('#')
        .next()
        .map(str::trim)
        .ok_or_else(|| anyhow!("unexpected cargo search output: {first_line}"))?;
    let version = quoted.trim_matches('"');
    Version::parse(version)
        .with_context(|| format!("failed to parse crate version from `{first_line}`"))?;
    Ok(version.to_string())
}

#[async_trait]
trait ToolchainManifestSource {
    async fn load_manifest(
        &self,
        repo: &str,
        tag: &str,
        token: Option<&str>,
    ) -> Result<Option<ToolchainManifest>>;
}

struct OciToolchainManifestSource;

#[async_trait]
impl ToolchainManifestSource for OciToolchainManifestSource {
    async fn load_manifest(
        &self,
        repo: &str,
        tag: &str,
        token: Option<&str>,
    ) -> Result<Option<ToolchainManifest>> {
        let auth = optional_registry_auth(token)?;
        let client = oci_client();
        let reference = parse_reference(repo, tag)?;
        let image = match client
            .pull(&reference, &auth, vec![TOOLCHAIN_LAYER_MEDIA_TYPE])
            .await
        {
            Ok(image) => image,
            Err(err) if is_missing_manifest_error(&err) || is_unauthorized_error(&err) => {
                return Ok(None);
            }
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("failed to pull {}", toolchain_ref(repo, tag)));
            }
        };
        let Some(layer) = image
            .layers
            .into_iter()
            .find(|layer| layer.media_type == TOOLCHAIN_LAYER_MEDIA_TYPE)
        else {
            return Ok(None);
        };
        let manifest = serde_json::from_slice::<ToolchainManifest>(&layer.data)
            .with_context(|| format!("failed to parse {}", toolchain_ref(repo, tag)))?;
        validate_manifest(&manifest)?;
        Ok(Some(manifest))
    }
}

async fn load_source_manifest(
    repo: &str,
    tag: &str,
    token: Option<&str>,
) -> Result<Option<ToolchainManifest>> {
    OciToolchainManifestSource
        .load_manifest(repo, tag, token)
        .await
}

fn oci_client() -> Client {
    Client::new(ClientConfig {
        protocol: ClientProtocol::Https,
        ..Default::default()
    })
}

fn registry_auth(raw_token: Option<&str>) -> Result<RegistryAuth> {
    let token = resolve_registry_token(raw_token)?
        .or_else(|| std::env::var("GHCR_TOKEN").ok())
        .or_else(|| std::env::var("GITHUB_TOKEN").ok())
        .context("GHCR token is required; pass --token or set GHCR_TOKEN/GITHUB_TOKEN")?;
    if token.trim().is_empty() {
        bail!("GHCR token is empty");
    }
    Ok(RegistryAuth::Basic(DEFAULT_OAUTH_USER.to_string(), token))
}

fn optional_registry_auth(raw_token: Option<&str>) -> Result<RegistryAuth> {
    match registry_auth(raw_token) {
        Ok(auth) => Ok(auth),
        Err(_) if raw_token.is_none() => Ok(RegistryAuth::Anonymous),
        Err(err) => Err(err),
    }
}

fn resolve_registry_token(raw_token: Option<&str>) -> Result<Option<String>> {
    let Some(raw_token) = raw_token else {
        return Ok(None);
    };
    if let Some(var) = raw_token.strip_prefix("env:") {
        let token =
            std::env::var(var).with_context(|| format!("failed to resolve env var {var}"))?;
        if token.trim().is_empty() {
            bail!("env var {var} resolved to an empty token");
        }
        return Ok(Some(token));
    }
    if raw_token.trim().is_empty() {
        bail!("GHCR token is empty");
    }
    Ok(Some(raw_token.to_string()))
}

fn parse_reference(repo: &str, tag: &str) -> Result<Reference> {
    Reference::from_str(&toolchain_ref(repo, tag))
        .with_context(|| format!("invalid OCI reference `{}`", toolchain_ref(repo, tag)))
}

async fn manifest_exists(
    client: &Client,
    reference: &Reference,
    auth: &RegistryAuth,
) -> Result<bool> {
    match client.pull_manifest(reference, auth).await {
        Ok(_) => Ok(true),
        Err(err) if is_missing_manifest_error(&err) => Ok(false),
        Err(err) => Err(err).context("failed to check whether release tag exists"),
    }
}

fn is_missing_manifest_error(err: &oci_distribution::errors::OciDistributionError) -> bool {
    let msg = err.to_string().to_ascii_lowercase();
    msg.contains("manifest unknown")
        || msg.contains("name unknown")
        || msg.contains("not found")
        || msg.contains("404")
}

fn is_unauthorized_error(err: &oci_distribution::errors::OciDistributionError) -> bool {
    let msg = err.to_string().to_ascii_lowercase();
    msg.contains("not authorized") || msg.contains("unauthorized") || msg.contains("401")
}

async fn push_manifest_layer(
    client: &Client,
    reference: &Reference,
    auth: &RegistryAuth,
    manifest: &ToolchainManifest,
) -> Result<()> {
    let data = serde_json::to_vec_pretty(manifest).context("failed to serialize manifest")?;
    let layer = ImageLayer::new(data, TOOLCHAIN_LAYER_MEDIA_TYPE.to_string(), None);
    let config = Config::new(
        br#"{"toolchain":"gtc"}"#.to_vec(),
        TOOLCHAIN_CONFIG_MEDIA_TYPE.to_string(),
        None,
    );
    client
        .push(reference, &[layer], config, auth, None)
        .await
        .context("failed to push toolchain manifest")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedResolver;

    impl CrateVersionResolver for FixedResolver {
        fn resolve_latest(&self, crate_name: &str) -> Result<String> {
            Ok(match crate_name {
                "greentic-runner" => "0.5.10",
                _ => "1.2.3",
            }
            .to_string())
        }
    }

    #[test]
    fn parses_cargo_search_version() {
        let version = parse_cargo_search_version(
            "greentic-dev",
            r#"greentic-dev = "0.5.1"    # Developer CLI"#,
        )
        .unwrap();
        assert_eq!(version, "0.5.1");
    }

    #[test]
    fn generates_manifest_from_catalogue() {
        let manifest = generate_manifest("1.0.5", "dev", None, &FixedResolver, None).unwrap();
        assert_eq!(manifest.schema, TOOLCHAIN_MANIFEST_SCHEMA);
        assert_eq!(manifest.toolchain, TOOLCHAIN_NAME);
        assert_eq!(manifest.version, "1.0.5");
        assert_eq!(manifest.channel.as_deref(), Some("dev"));
        assert!(
            manifest
                .packages
                .iter()
                .any(|package| package.crate_name == "greentic-bundle"
                    && package.bins == ["greentic-bundle"])
        );
        assert!(
            manifest
                .packages
                .iter()
                .any(|package| package.crate_name == "greentic-runner"
                    && package.bins == ["greentic-runner"])
        );
    }

    #[test]
    fn source_manifest_can_pin_package_versions() {
        let source = ToolchainManifest {
            schema: TOOLCHAIN_MANIFEST_SCHEMA.to_string(),
            toolchain: TOOLCHAIN_NAME.to_string(),
            version: "dev".to_string(),
            channel: Some("dev".to_string()),
            created_at: None,
            packages: vec![ToolchainPackage {
                crate_name: "greentic-dev".to_string(),
                bins: vec!["greentic-dev".to_string()],
                version: "0.5.9".to_string(),
            }],
        };
        let manifest =
            generate_manifest("1.0.5", "dev", Some(&source), &FixedResolver, None).unwrap();
        let greentic_dev = manifest
            .packages
            .iter()
            .find(|package| package.crate_name == "greentic-dev")
            .unwrap();
        assert_eq!(greentic_dev.version, "0.5.9");
    }

    #[test]
    fn bootstrap_source_manifest_uses_source_tag_identity() {
        let manifest = bootstrap_source_manifest("dev", &FixedResolver, None).unwrap();
        assert_eq!(manifest.version, "dev");
        assert_eq!(manifest.channel.as_deref(), Some("dev"));
        assert_eq!(manifest.schema, TOOLCHAIN_MANIFEST_SCHEMA);
        assert_eq!(manifest.toolchain, TOOLCHAIN_NAME);
        assert!(
            manifest
                .packages
                .iter()
                .all(|package| package.version != "latest")
        );
    }

    #[test]
    fn validates_schema_and_toolchain() {
        let mut manifest = generate_manifest("1.0.5", "dev", None, &FixedResolver, None).unwrap();
        assert!(validate_manifest(&manifest).is_ok());
        manifest.schema = "wrong".to_string();
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn resolves_inline_registry_token() {
        assert_eq!(
            resolve_registry_token(Some("secret-token"))
                .unwrap()
                .as_deref(),
            Some("secret-token")
        );
    }

    #[test]
    fn release_view_tag_prefers_release_or_tag() {
        let args = ReleaseViewArgs {
            release: Some("1.0.5".to_string()),
            tag: None,
            repo: "ghcr.io/greenticai/greentic-versions/gtc".to_string(),
            token: None,
        };
        assert_eq!(release_view_tag(&args).unwrap(), "1.0.5");

        let args = ReleaseViewArgs {
            release: None,
            tag: Some("stable".to_string()),
            repo: "ghcr.io/greenticai/greentic-versions/gtc".to_string(),
            token: None,
        };
        assert_eq!(release_view_tag(&args).unwrap(), "stable");
    }

    #[test]
    fn publish_manifest_input_uses_local_manifest_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gtc-1.0.12.json");
        let manifest = generate_manifest("1.0.12", "dev", None, &FixedResolver, None).unwrap();
        fs::write(&path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
        let args = ReleasePublishArgs {
            release: None,
            from: None,
            tag: Some("stable".to_string()),
            manifest: Some(path.clone()),
            repo: "ghcr.io/greenticai/greentic-versions/gtc".to_string(),
            token: None,
            out: dir.path().to_path_buf(),
            dry_run: true,
            force: true,
        };
        let (release, loaded, source_path) = publish_manifest_input(&args).unwrap();
        assert_eq!(release, "1.0.12");
        assert_eq!(loaded, manifest);
        assert_eq!(
            source_path,
            Some(PublishManifestSource::Local(path.clone()))
        );
    }

    #[test]
    fn publish_manifest_input_allows_release_override_for_local_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gtc-1.0.13.json");
        let manifest = generate_manifest("1.0.12", "dev", None, &FixedResolver, None).unwrap();
        fs::write(&path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
        let args = ReleasePublishArgs {
            release: Some("1.0.13".to_string()),
            from: None,
            tag: Some("stable".to_string()),
            manifest: Some(path.clone()),
            repo: "ghcr.io/greenticai/greentic-versions/gtc".to_string(),
            token: None,
            out: dir.path().to_path_buf(),
            dry_run: true,
            force: true,
        };
        let (release, loaded, source_path) = publish_manifest_input(&args).unwrap();
        assert_eq!(release, "1.0.13");
        assert_eq!(loaded.version, "1.0.13");
        assert_eq!(
            source_path,
            Some(PublishManifestSource::Local(path.clone()))
        );
    }

    #[test]
    fn latest_dev_manifest_uses_latest_versions() {
        let manifest = latest_dev_manifest(None);
        assert_eq!(manifest.version, "dev");
        assert_eq!(manifest.channel.as_deref(), Some("dev"));
        assert_eq!(manifest.schema, TOOLCHAIN_MANIFEST_SCHEMA);
        assert_eq!(manifest.toolchain, TOOLCHAIN_NAME);
        assert!(!manifest.packages.is_empty());
        assert!(
            manifest
                .packages
                .iter()
                .all(|package| package.version == "latest")
        );
    }

    #[test]
    fn builds_toolchain_ref() {
        assert_eq!(
            toolchain_ref("ghcr.io/greenticai/greentic-versions/gtc", "stable"),
            "ghcr.io/greenticai/greentic-versions/gtc:stable"
        );
    }
}
