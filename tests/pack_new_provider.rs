use assert_cmd::cargo::cargo_bin_cmd;
use greentic_types::provider::{ProviderExtensionInline, ProviderRuntimeRef};
use greentic_types::{PackId, PackKind, PackManifest};
use semver::Version;
use tempfile::tempdir;

fn base_manifest() -> PackManifest {
    PackManifest {
        schema_version: "1".into(),
        pack_id: PackId::new("dev.local.test").unwrap(),
        version: Version::parse("0.1.0").unwrap(),
        kind: PackKind::Application,
        publisher: "test".into(),
        components: Vec::new(),
        flows: Vec::new(),
        dependencies: Vec::new(),
        capabilities: Vec::new(),
        secret_requirements: Vec::new(),
        signatures: Default::default(),
        bootstrap: None,
        extensions: None,
    }
}

fn write_manifest(path: &std::path::Path, manifest: &PackManifest) {
    let bytes = greentic_types::encode_pack_manifest(manifest).expect("encode manifest");
    std::fs::write(path, bytes).expect("write manifest");
}

fn read_manifest(path: &std::path::Path) -> PackManifest {
    let bytes = std::fs::read(path).expect("read manifest");
    greentic_types::decode_pack_manifest(&bytes).expect("decode manifest")
}

fn add_provider_cmd(args: &[&str]) -> assert_cmd::Command {
    let mut cmd = cargo_bin_cmd!("greentic-dev");
    cmd.args(args);
    cmd
}

fn provider_extension_inline(manifest: &PackManifest) -> ProviderExtensionInline {
    if let Some(inline) = manifest
        .extensions
        .as_ref()
        .and_then(|exts| exts.get("greentic.provider-extension.v1"))
        .and_then(|ext| ext.inline.as_ref())
    {
        match inline {
            greentic_types::pack_manifest::ExtensionInline::Provider(value) => value.clone(),
            greentic_types::pack_manifest::ExtensionInline::Other(value) => {
                serde_json::from_value(value.clone()).unwrap_or_default()
            }
        }
    } else {
        ProviderExtensionInline::default()
    }
}

#[test]
fn adds_provider_extension_entry() {
    let tmp = tempdir().unwrap();
    let manifest_path = tmp.path().join("manifest.cbor");
    write_manifest(&manifest_path, &base_manifest());

    add_provider_cmd(&[
        "pack",
        "new-provider",
        "--pack",
        manifest_path.to_str().unwrap(),
        "--id",
        "vendor.db",
        "--runtime",
        "vendor.db.runtime::greentic_provider@greentic:provider/runtime",
        "--manifest",
        "providers/vendor.db/provider.yaml",
        "--kind",
        "database",
    ])
    .assert()
    .success();

    let manifest = read_manifest(&manifest_path);
    let inline = provider_extension_inline(&manifest);
    assert_eq!(inline.providers.len(), 1);
    let provider = &inline.providers[0];
    assert_eq!(provider.provider_type, "vendor.db");
    assert_eq!(
        provider.config_schema_ref,
        "providers/vendor.db/provider.yaml"
    );
    assert_eq!(provider.capabilities, vec!["database".to_string()]);
    let runtime = ProviderRuntimeRef {
        component_ref: "vendor.db.runtime".into(),
        export: "greentic_provider".into(),
        world: "greentic:provider/runtime".into(),
    };
    assert_eq!(provider.runtime, runtime);
}

#[test]
fn duplicate_id_requires_force() {
    let tmp = tempdir().unwrap();
    let manifest_path = tmp.path().join("manifest.cbor");
    write_manifest(&manifest_path, &base_manifest());

    let args = [
        "pack",
        "new-provider",
        "--pack",
        manifest_path.to_str().unwrap(),
        "--id",
        "vendor.db",
        "--runtime",
        "vendor.db.runtime::greentic_provider@greentic:provider/runtime",
    ];
    add_provider_cmd(&args).assert().success();
    add_provider_cmd(&args).assert().failure();
}

#[test]
fn force_updates_existing_provider() {
    let tmp = tempdir().unwrap();
    let manifest_path = tmp.path().join("manifest.cbor");
    write_manifest(&manifest_path, &base_manifest());

    add_provider_cmd(&[
        "pack",
        "new-provider",
        "--pack",
        manifest_path.to_str().unwrap(),
        "--id",
        "vendor.db",
        "--runtime",
        "vendor.db.runtime::greentic_provider@greentic:provider/runtime",
    ])
    .assert()
    .success();

    add_provider_cmd(&[
        "pack",
        "new-provider",
        "--pack",
        manifest_path.to_str().unwrap(),
        "--id",
        "vendor.db",
        "--runtime",
        "vendor.db.runtime::updated@greentic:provider/runtime",
        "--force",
    ])
    .assert()
    .success();

    let manifest = read_manifest(&manifest_path);
    let provider = &provider_extension_inline(&manifest).providers[0];
    assert_eq!(
        provider.runtime,
        ProviderRuntimeRef {
            component_ref: "vendor.db.runtime".into(),
            export: "updated".into(),
            world: "greentic:provider/runtime".into(),
        }
    );
}

#[test]
fn dry_run_does_not_modify_manifest() {
    let tmp = tempdir().unwrap();
    let manifest_path = tmp.path().join("manifest.cbor");
    let base = base_manifest();
    write_manifest(&manifest_path, &base);
    let before = std::fs::read(&manifest_path).expect("read before");

    add_provider_cmd(&[
        "pack",
        "new-provider",
        "--pack",
        manifest_path.to_str().unwrap(),
        "--id",
        "vendor.db",
        "--runtime",
        "vendor.db.runtime::greentic_provider@greentic:provider/runtime",
        "--dry-run",
    ])
    .assert()
    .success();

    let after = std::fs::read(&manifest_path).expect("read after");
    assert_eq!(before, after, "dry-run should not mutate manifest");
}

#[test]
fn scaffolds_provider_manifest_when_requested() {
    let tmp = tempdir().unwrap();
    let pack_root = tmp.path();
    let manifest_path = pack_root.join("manifest.cbor");
    write_manifest(&manifest_path, &base_manifest());

    add_provider_cmd(&[
        "pack",
        "new-provider",
        "--pack",
        manifest_path.to_str().unwrap(),
        "--id",
        "vendor.db",
        "--runtime",
        "vendor.db.runtime::greentic_provider@greentic:provider/runtime",
        "--kind",
        "database",
        "--scaffold-files",
    ])
    .assert()
    .success();

    let manifest = read_manifest(&manifest_path);
    let provider = &provider_extension_inline(&manifest).providers[0];
    let provider_yaml = pack_root.join(&provider.config_schema_ref);
    assert!(
        provider_yaml.exists(),
        "expected scaffolded provider manifest at {}",
        provider_yaml.display()
    );
}
