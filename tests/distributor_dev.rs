use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use greentic_dev::pack_init::{PackInitIntent, run, run_component_add};
use httpmock::MockServer;
use once_cell::sync::Lazy;
use serde_json::json;
use std::sync::Mutex;
use tempfile::tempdir;

static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

fn write_config(base: &PathBuf, url: &str) -> PathBuf {
    let config_dir = base.join(".greentic");
    fs::create_dir_all(&config_dir).unwrap();
    let path = config_dir.join("config.toml");
    fs::write(
        &path,
        format!(
            r#"
[distributor.default]
url = "{url}"
token = ""
"#
        ),
    )
    .unwrap();
    path
}

#[test]
fn component_add_updates_manifest_and_cache() -> Result<()> {
    let _guard = ENV_LOCK.lock().unwrap();
    if std::net::TcpListener::bind("127.0.0.1:0").is_err() {
        eprintln!("Skipping test; cannot bind local port in this environment");
        return Ok(());
    }

    let temp_home = tempdir().unwrap();
    unsafe { std::env::set_var("HOME", temp_home.path()) };
    let config_path = write_config(&PathBuf::from(temp_home.path()), "http://localhost:5000");
    unsafe { std::env::set_var("GREENTIC_CONFIG", &config_path) };
    let workspace = tempdir().unwrap();
    std::env::set_current_dir(workspace.path()).unwrap();

    let server = MockServer::start();
    let resolve_body = json!({
        "kind": "component",
        "name": "component-llm-openai",
        "version": "0.3.2",
        "coordinate": "component://greentic/component-llm-openai@^0.3",
        "artifact_id": "artifacts/sha256:abc123",
        "artifact_download_path": "/v1/artifact/artifacts/sha256:abc123",
        "digest": "sha256:abc123",
        "license": {
            "license_type": "free",
            "id": null,
            "requires_acceptance": false,
            "checkout_url": null
        },
        "metadata": {}
    });
    server.mock(|when, then| {
        when.method("POST").path("/v1/resolve");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(resolve_body.clone());
    });
    server.mock(|when, then| {
        when.method("GET")
            .path("/v1/artifact/artifacts/sha256:abc123");
        then.status(200).body("wasm-bytes");
    });

    // point config to mock server
    fs::write(
        &config_path,
        format!(
            r#"
[distributor.default]
url = "{}"
token = ""
"#,
            server.base_url()
        ),
    )
    .unwrap();

    run_component_add(
        "component://greentic/component-llm-openai@^0.3",
        None,
        PackInitIntent::Dev,
    )
    .unwrap();

    let manifest_path = workspace.path().join(".greentic/manifest.json");
    let manifest_raw = fs::read_to_string(&manifest_path).unwrap();
    assert!(
        manifest_raw.contains("component-llm-openai"),
        "manifest should include component entry"
    );

    let cache_path = temp_home
        .path()
        .join(".greentic/cache/components/sha256-abc123/artifact.wasm");
    assert!(cache_path.exists(), "cached artifact should exist");
    Ok(())
}

#[test]
fn pack_init_creates_workspace() -> Result<()> {
    let _guard = ENV_LOCK.lock().unwrap();
    if std::net::TcpListener::bind("127.0.0.1:0").is_err() {
        eprintln!("Skipping test; cannot bind local port in this environment");
        return Ok(());
    }

    let temp_home = tempdir().unwrap();
    unsafe { std::env::set_var("HOME", temp_home.path()) };
    let config_path = write_config(&PathBuf::from(temp_home.path()), "http://localhost:5000");
    unsafe { std::env::set_var("GREENTIC_CONFIG", &config_path) };
    let workspace = tempdir().unwrap();
    std::env::set_current_dir(workspace.path()).unwrap();

    let server = MockServer::start();
    let resolve_body = json!({
        "kind": "pack",
        "name": "demo-pack",
        "version": "1.0.0",
        "coordinate": "pack://org/demo-pack@1.0.0",
        "artifact_id": "artifacts/sha256:pack123",
        "artifact_download_path": "/v1/artifact/artifacts/sha256:pack123",
        "digest": "sha256:pack123",
        "license": {
            "license_type": "free",
            "id": null,
            "requires_acceptance": false,
            "checkout_url": null
        },
        "metadata": {}
    });
    server.mock(|when, then| {
        when.method("POST").path("/v1/resolve");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(resolve_body.clone());
    });

    // build minimal zip bytes for artifact
    let mut data: Vec<u8> = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut data);
        let mut zip = zip::ZipWriter::new(cursor);
        let opts = zip::write::FileOptions::default();
        zip.add_directory("flows/", opts).unwrap();
        zip.start_file("flows/demo/flow.ygtc", opts).unwrap();
        zip.write_all(b"flow").unwrap();
        zip.finish().unwrap();
    }

    server.mock(|when, then| {
        when.method("GET")
            .path("/v1/artifact/artifacts/sha256:pack123");
        then.status(200).body(data.clone());
    });

    fs::write(
        &config_path,
        format!(
            r#"
[distributor.default]
url = "{}"
token = ""
"#,
            server.base_url()
        ),
    )
    .unwrap();

    run("pack://org/demo-pack@1.0.0", None).unwrap();

    let dest = workspace.path().join("demo-pack");
    assert!(
        dest.exists() && dest.is_dir(),
        "workspace directory should exist"
    );
    assert!(
        dest.join("bundle.gtpack").exists(),
        "bundle file should be written"
    );
    Ok(())
}
