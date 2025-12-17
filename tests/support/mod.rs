#![allow(dead_code)]

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use blake3::Hasher;
use greentic_dev::pack_build::{self, PackSigning};
use greentic_pack::builder::PackManifest;
use serde_json::json;
use tempfile::TempDir;
use walkdir::WalkDir;
use zip::ZipArchive;

pub mod custom_add_step;
pub mod exec;
pub mod flow;
pub mod l3;
pub mod l4;
pub mod real_weather;

pub static WORKDIR_LOCK: Mutex<()> = Mutex::new(());

pub struct Workspace {
    pub root: PathBuf,
    keep: bool,
    _tempdir: Option<TempDir>,
}

impl Workspace {
    pub fn new(prefix: &str) -> Result<Self> {
        let keep = keep_artifacts();
        let tmp = tempfile::Builder::new()
            .prefix(prefix)
            .tempdir()
            .context("failed to create temp workspace")?;
        let root = tmp.path().to_path_buf();
        let _tempdir = if keep {
            #[allow(deprecated)]
            let path = tmp.into_path();
            fs::create_dir_all(&path)
                .with_context(|| format!("failed to ensure {}", path.display()))?;
            None
        } else {
            Some(tmp)
        };
        Ok(Self {
            root,
            keep,
            _tempdir,
        })
    }
}

fn keep_artifacts() -> bool {
    matches!(
        std::env::var("KEEP_TEST_ARTIFACTS")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes"
    )
}

#[allow(dead_code)]
pub fn copy_fixture_component(workspace: &Workspace, missing_default: bool) -> Result<PathBuf> {
    copy_fixture_component_with_schema(workspace, missing_default, None)
}

#[allow(dead_code)]
pub fn copy_fixture_component_with_schema(
    workspace: &Workspace,
    missing_default: bool,
    config_schema: Option<serde_json::Value>,
) -> Result<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture = root.join("fixtures/components/dev.greentic.echo");
    let dest = workspace.root.join("components").join("dev.greentic.echo");
    copy_dir(&fixture, &dest)?;

    let manifest_path = dest.join("component.manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path)?)
            .context("invalid component manifest JSON")?;

    if missing_default {
        if let Some(obj) = manifest.as_object_mut() {
            obj.remove("dev_flows");
        }
    } else {
        ensure_default_dev_flow(&mut manifest);
    }

    if let Some(schema) = config_schema {
        manifest
            .as_object_mut()
            .expect("manifest object")
            .insert("config_schema".to_string(), schema);
    }

    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).context("serialize manifest")?,
    )
    .with_context(|| format!("failed to write {}", manifest_path.display()))?;

    Ok(dest)
}

pub fn write_pack_flow(workspace: &Workspace, flow_id: &str) -> Result<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_flow = root.join("tests/fixtures/hello-pack/hello-flow.ygtc");
    let flow_dir = workspace.root.join("flows");
    fs::create_dir_all(&flow_dir)
        .with_context(|| format!("failed to create {}", flow_dir.display()))?;
    let dest = flow_dir.join(format!("{flow_id}.ygtc"));
    fs::copy(&fixture_flow, &dest)
        .with_context(|| format!("failed to copy flow to {}", dest.display()))?;
    Ok(dest)
}

pub fn build_pack(
    workspace: &Workspace,
    flow_path: &Path,
    component_dir: &Path,
) -> Result<PathBuf> {
    let _guard = WORKDIR_LOCK.lock().unwrap();
    let prev = std::env::current_dir().context("current_dir")?;
    std::env::set_current_dir(&workspace.root).context("set cwd")?;

    let pack_path = workspace.root.join("dist").join("test.gtpack");
    pack_build::run(
        flow_path,
        &pack_path,
        PackSigning::Dev,
        None,
        Some(component_dir),
    )
    .context("pack build")?;

    std::env::set_current_dir(prev).context("restore cwd")?;
    Ok(pack_path)
}

#[allow(dead_code)]
pub fn load_gtpack(gtpack_path: &Path) -> Result<(PackManifest, HashMap<String, Vec<u8>>)> {
    let file = fs::File::open(gtpack_path)
        .with_context(|| format!("failed to open {}", gtpack_path.display()))?;
    let mut archive = ZipArchive::new(file).context("failed to open gtpack zip")?;
    let mut manifest_bytes = Vec::new();
    archive
        .by_name("manifest.cbor")
        .context("manifest.cbor missing")?
        .read_to_end(&mut manifest_bytes)
        .context("failed to read manifest.cbor")?;
    let manifest: PackManifest =
        serde_cbor::from_slice(&manifest_bytes).context("decode manifest")?;

    let mut components = HashMap::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).context("gtpack entry")?;
        let mut buf = Vec::new();
        entry
            .read_to_end(&mut buf)
            .with_context(|| format!("read {}", entry.name()))?;
        components.insert(entry.name().to_string(), buf);
    }

    Ok((manifest, components))
}

pub fn compute_blake3_hex(bytes: &[u8]) -> String {
    let mut hasher = Hasher::new();
    hasher.update(bytes);
    let hash = hasher.finalize();
    hash.to_hex().to_string()
}

fn ensure_default_dev_flow(manifest: &mut serde_json::Value) {
    let graph = json!({
        "schema_version": 1,
        "id": "component.default",
        "type": "component-config",
        "start": "emit_config",
        "nodes": {
            "emit_config": {
                "template": "{ \"node_id\": \"qa_step\", \"node\": { \"qa\": { \"component\": \"component-qa-process\", \"question\": \"hi\" }, \"routing\": [{ \"to\": \"NEXT_NODE_PLACEHOLDER\" }] } }"
            }
        },
        "edges": []
    });

    let flows = manifest
        .as_object_mut()
        .expect("manifest should be object")
        .entry("dev_flows")
        .or_insert_with(|| json!({}));
    let map = flows.as_object_mut().expect("dev_flows object");
    map.insert(
        "default".to_string(),
        json!({
            "format": "flow-ir-json",
            "graph": graph,
        }),
    );
}

pub fn diag(test: &str, stage: &str, workspace: &Workspace, note: &str) {
    diag_with_owner(test, stage, workspace, note, "greentic-dev");
}

pub fn diag_with_owner(test: &str, stage: &str, workspace: &Workspace, note: &str, owner: &str) {
    eprintln!(
        "\n--- realism-diag ---\n\
         test: {test}\n\
         stage: {stage}\n\
         owner: {owner}\n\
         repro: KEEP_TEST_ARTIFACTS=1 cargo test {test} -- --nocapture\n\
        workspace: {}\n\
        note: {note}\n\
        keep: {}\n---------------------",
        workspace.root.display(),
        workspace.keep
    );
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    for entry in WalkDir::new(src) {
        let entry = entry?;
        let path = entry.path();
        let relative = path.strip_prefix(src).context("strip prefix")?;
        let target = dst.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)
                .with_context(|| format!("failed to create {}", target.display()))?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            fs::copy(path, &target).with_context(|| {
                format!("failed to copy {} to {}", path.display(), target.display())
            })?;
        }
    }
    Ok(())
}
