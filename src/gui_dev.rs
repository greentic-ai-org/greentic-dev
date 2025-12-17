use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::json;
use which::which;

use crate::cli::{GuiPackDevArgs, GuiPackKind, GuiServeArgs};

const DEFAULT_BIND: &str = "127.0.0.1:8080";
const DEFAULT_DOMAIN: &str = "localhost:8080";

#[derive(Debug, Deserialize)]
pub struct GuiDevConfig {
    pub tenant: String,
    #[serde(default = "default_domain")]
    pub domain: String,
    #[serde(default)]
    pub bind: Option<String>,
    pub layout_pack: PathBuf,
    #[serde(default)]
    pub auth_pack: Option<PathBuf>,
    #[serde(default)]
    pub skin_pack: Option<PathBuf>,
    #[serde(default)]
    pub telemetry_pack: Option<PathBuf>,
    #[serde(default)]
    pub feature_packs: Vec<PathBuf>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub worker_overrides: HashMap<String, String>,
}

fn default_domain() -> String {
    DEFAULT_DOMAIN.to_string()
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind")]
enum GuiManifest {
    #[serde(rename = "gui-layout")]
    Layout { layout: LayoutSection },
    #[serde(rename = "gui-feature")]
    Feature { routes: Vec<FeatureRoute> },
    #[serde(rename = "gui-auth")]
    Auth { routes: Vec<AuthRoute> },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
struct LayoutSection {
    entrypoint_html: String,
    #[serde(default)]
    #[allow(dead_code)]
    slots: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct FeatureRoute {
    path: String,
    #[serde(default)]
    authenticated: bool,
}

#[derive(Debug, Deserialize)]
struct AuthRoute {
    path: String,
    #[serde(default)]
    public: bool,
}

pub fn run_gui_command(cmd: crate::cli::GuiCommand) -> Result<()> {
    match cmd {
        crate::cli::GuiCommand::Serve(args) => run_gui_serve(&args),
        crate::cli::GuiCommand::PackDev(args) => run_pack_dev(&args),
    }
}

fn run_gui_serve(args: &GuiServeArgs) -> Result<()> {
    let config_path = resolve_config_path(args.config.as_deref())?;
    let config = load_config(&config_path)?;
    validate_config(&config)?;

    let bind = args
        .bind
        .as_deref()
        .or(config.bind.as_deref())
        .unwrap_or(DEFAULT_BIND);
    let domain = args.domain.as_deref().unwrap_or(&config.domain);

    println!(
        "Starting greentic-gui for tenant {} on http://{} (bind {})",
        config.tenant, domain, bind
    );
    summarize_routes(&config);

    let mut command = if let Some(gui_bin) = args.gui_bin.as_ref() {
        Command::new(gui_bin)
    } else if let Ok(bin) = which("greentic-gui") {
        Command::new(bin)
    } else if args.no_cargo_fallback {
        bail!("greentic-gui binary not found on PATH and cargo fallback disabled");
    } else {
        println!("greentic-gui not found on PATH; falling back to `cargo run -p greentic-gui`");
        let mut cmd = Command::new("cargo");
        cmd.args(["run", "-p", "greentic-gui", "--"]);
        cmd
    };

    command
        .arg("--config")
        .arg(&config_path)
        .arg("--bind")
        .arg(bind)
        .arg("--domain")
        .arg(domain)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let mut child = command.spawn().context("failed to launch greentic-gui")?;

    if args.open_browser {
        let _ = open_browser(&format!("http://{}", bind));
    }

    child.wait().context("greentic-gui exited abnormally")?;
    Ok(())
}

fn summarize_routes(config: &GuiDevConfig) {
    let mut routes = Vec::new();
    if let Some(route) = extract_layout_route(&config.layout_pack) {
        routes.push(route);
    }
    if let Some(path) = config.auth_pack.as_ref() {
        routes.extend(extract_auth_routes(path));
    }
    for feature in &config.feature_packs {
        routes.extend(extract_feature_routes(feature));
    }
    if routes.is_empty() {
        println!("Routes: (none detected from manifests)");
    } else {
        println!("Routes:");
        for route in routes {
            println!("  - {}", route);
        }
    }
}

fn extract_layout_route(pack: &Path) -> Option<String> {
    read_manifest(pack).and_then(|manifest| match manifest {
        GuiManifest::Layout { layout } => {
            Some(format!("/ (entrypoint {})", layout.entrypoint_html))
        }
        _ => None,
    })
}

fn extract_auth_routes(pack: &Path) -> Vec<String> {
    match read_manifest(pack) {
        Some(GuiManifest::Auth { routes }) => routes
            .into_iter()
            .map(|route| {
                let visibility = if route.public { "public" } else { "auth" };
                format!("{} (auth: {})", route.path, visibility)
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn extract_feature_routes(pack: &Path) -> Vec<String> {
    match read_manifest(pack) {
        Some(GuiManifest::Feature { routes }) => routes
            .into_iter()
            .map(|route| {
                let visibility = if route.authenticated {
                    "auth"
                } else {
                    "public"
                };
                format!("{} (feature: {})", route.path, visibility)
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn read_manifest(pack_path: &Path) -> Option<GuiManifest> {
    if !pack_path.is_dir() {
        return None;
    }
    let manifest_path = pack_path.join("gui").join("manifest.json");
    let data = fs::read_to_string(manifest_path).ok()?;
    serde_json::from_str(&data).ok()
}

pub fn resolve_config_path(cli_override: Option<&Path>) -> Result<PathBuf> {
    let mut searched = Vec::new();
    if let Some(override_path) = cli_override {
        if override_path.exists() {
            return Ok(override_path.to_path_buf());
        }
        bail!(
            "provided gui-dev config {} does not exist",
            override_path.display()
        );
    }

    let cwd = std::env::current_dir().context("unable to read current directory")?;
    let candidates = [
        cwd.join("gui-dev.yaml"),
        cwd.join(".greentic").join("gui-dev.yaml"),
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("/nonexistent"))
            .join("greentic-dev")
            .join("gui-dev.yaml"),
    ];
    for candidate in candidates {
        searched.push(candidate.clone());
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    bail!(
        "no gui-dev.yaml found; looked in: {}",
        searched
            .into_iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn load_config(path: &Path) -> Result<GuiDevConfig> {
    let data = fs::read_to_string(path)
        .with_context(|| format!("failed to read gui-dev config at {}", path.display()))?;
    let mut config: GuiDevConfig = serde_yaml_bw::from_str(&data)
        .with_context(|| format!("failed to parse gui-dev config at {}", path.display()))?;
    if config.domain.is_empty() {
        config.domain = DEFAULT_DOMAIN.to_string();
    }
    Ok(config)
}

fn validate_config(config: &GuiDevConfig) -> Result<()> {
    ensure_path(&config.layout_pack, "layout_pack")?;
    if let Some(path) = &config.auth_pack {
        ensure_path(path, "auth_pack")?;
    }
    if let Some(path) = &config.skin_pack {
        ensure_path(path, "skin_pack")?;
    }
    if let Some(path) = &config.telemetry_pack {
        ensure_path(path, "telemetry_pack")?;
    }
    for (idx, path) in config.feature_packs.iter().enumerate() {
        ensure_path(path, &format!("feature_packs[{}]", idx))?;
    }
    Ok(())
}

fn ensure_path(path: &Path, label: &str) -> Result<()> {
    if !path.exists() {
        bail!("{} path {} does not exist", label, path.display());
    }
    Ok(())
}

fn run_pack_dev(args: &GuiPackDevArgs) -> Result<()> {
    if let Some(cmd) = args.build_cmd.as_ref()
        && !args.no_build
    {
        run_build_cmd(cmd, &args.dir)?;
    }

    stage_pack(args)?;
    Ok(())
}

fn run_build_cmd(cmd: &str, dir: &Path) -> Result<()> {
    println!("Running build command: {}", cmd);
    #[cfg(target_os = "windows")]
    let mut command = Command::new("cmd");
    #[cfg(target_os = "windows")]
    command.args(["/C", cmd]);

    #[cfg(not(target_os = "windows"))]
    let mut command = Command::new("sh");
    #[cfg(not(target_os = "windows"))]
    command.args(["-c", cmd]);

    command
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let status = command
        .status()
        .with_context(|| format!("failed to execute build command `{}`", cmd))?;
    if !status.success() {
        bail!("build command `{}` exited with {}", cmd, status);
    }
    Ok(())
}

fn stage_pack(args: &GuiPackDevArgs) -> Result<()> {
    let assets_dir = args.output.join("gui").join("assets");
    ensure_clean_dir(&assets_dir)?;
    copy_dir_recursive(&args.dir, &assets_dir)?;

    let manifest_path = args.output.join("gui").join("manifest.json");
    if let Some(provided) = args.manifest.as_ref() {
        fs::create_dir_all(
            manifest_path
                .parent()
                .expect("manifest has a parent directory"),
        )?;
        fs::copy(provided, &manifest_path).with_context(|| {
            format!(
                "failed to copy manifest from {} to {}",
                provided.display(),
                manifest_path.display()
            )
        })?;
    } else {
        let manifest = generate_manifest(args)?;
        fs::create_dir_all(manifest_path.parent().unwrap())?;
        fs::write(&manifest_path, manifest)
            .with_context(|| format!("failed to write manifest to {}", manifest_path.display()))?;
    }

    println!(
        "Staged GUI dev pack at {} (assets from {})",
        args.output.display(),
        args.dir.display()
    );
    Ok(())
}

fn ensure_clean_dir(path: &Path) -> Result<()> {
    if path.exists() {
        let meta = fs::metadata(path)
            .with_context(|| format!("failed to read existing path metadata {}", path.display()))?;
        if meta.is_file() {
            bail!("output path {} already exists as a file", path.display());
        }
        // Allow reusing existing directory; do not delete but ensure it is empty to avoid stale files.
        let mut entries =
            fs::read_dir(path).with_context(|| format!("failed to read {}", path.display()))?;
        if entries.next().is_some() {
            bail!(
                "output directory {} already exists and is not empty",
                path.display()
            );
        }
        return Ok(());
    }
    fs::create_dir_all(path)
        .with_context(|| format!("failed to create output directory {}", path.display()))
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    if !src.exists() {
        bail!("source directory {} does not exist", src.display());
    }
    for entry in
        fs::read_dir(src).with_context(|| format!("failed to read source {}", src.display()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            fs::create_dir_all(&dest_path)?;
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else if file_type.is_file() {
            fs::create_dir_all(
                dest_path
                    .parent()
                    .expect("destination file has a parent directory"),
            )?;
            fs::copy(entry.path(), &dest_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    entry.path().display(),
                    dest_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn generate_manifest(args: &GuiPackDevArgs) -> Result<String> {
    let manifest = match args.kind {
        GuiPackKind::Layout => json!({
            "kind": "gui-layout",
            "layout": {
                "slots": ["header", "menu", "main", "footer"],
                "entrypoint_html": format!("gui/assets/{}", args.entrypoint),
                "spa": true,
                "slot_selectors": {
                    "header": "#app-header",
                    "menu": "#app-menu",
                    "main": "#app-main",
                    "footer": "#app-footer"
                }
            }
        }),
        GuiPackKind::Feature => json!({
            "kind": "gui-feature",
            "routes": [{
                "path": args.feature_route.as_deref().unwrap_or("/"),
                "html": format!("gui/assets/{}", args.feature_html),
                "authenticated": args.feature_authenticated,
            }],
            "digital_workers": [],
            "fragments": []
        }),
    };
    serde_json::to_string_pretty(&manifest).context("failed to serialize manifest")
}

fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = Command::new("xdg-open");
    #[cfg(target_os = "windows")]
    let mut command = Command::new("cmd");

    #[cfg(target_os = "windows")]
    command.args(["/C", "start", url]);
    #[cfg(not(target_os = "windows"))]
    command.arg(url);

    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let _ = command.status();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn resolves_config_in_cwd_first() {
        let temp = TempDir::new().unwrap();
        let cwd = temp.path();
        let primary = cwd.join("gui-dev.yaml");
        fs::write(&primary, "tenant: test\nlayout_pack: ./layout").unwrap();

        let _guard = CurrentDirGuard::new(cwd);
        let path = resolve_config_path(None).unwrap().canonicalize().unwrap();
        let primary = primary.canonicalize().unwrap();
        assert_eq!(path, primary);
    }

    #[test]
    fn stages_layout_manifest() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("src");
        let out = temp.path().join("out");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("index.html"), "<html></html>").unwrap();

        let args = GuiPackDevArgs {
            dir: src.clone(),
            output: out.clone(),
            kind: GuiPackKind::Layout,
            entrypoint: "index.html".to_string(),
            manifest: None,
            feature_route: None,
            feature_html: "index.html".to_string(),
            feature_authenticated: false,
            build_cmd: None,
            no_build: true,
        };

        stage_pack(&args).unwrap();
        let manifest = fs::read_to_string(out.join("gui").join("manifest.json")).unwrap();
        let value: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        assert_eq!(value["kind"], "gui-layout");
        assert_eq!(value["layout"]["entrypoint_html"], "gui/assets/index.html");
        assert!(out.join("gui").join("assets").join("index.html").exists());
    }

    struct CurrentDirGuard {
        previous: PathBuf,
    }

    impl CurrentDirGuard {
        fn new(path: &Path) -> Self {
            let previous = std::env::current_dir().unwrap();
            std::env::set_current_dir(path).unwrap();
            CurrentDirGuard { previous }
        }
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.previous);
        }
    }
}
