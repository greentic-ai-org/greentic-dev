use std::fs;
use std::path::{Path, PathBuf};

use greentic_dev::cli::{ConfigFlowModeArg, FlowAddStepArgs};
use greentic_dev::flow_cmd::{parse_config_flow_output, render_config_flow_yaml, run_add_step};
use greentic_flow::flow_bundle::load_and_validate_bundle;
use serde_json::{Value as JsonValue, json};
use serde_yaml_bw as serde_yaml;
use std::sync::Mutex;

static WORKDIR_LOCK: Mutex<()> = Mutex::new(());

fn set_env(key: &str, value: &str) {
    unsafe { std::env::set_var(key, value) }
}

fn remove_env(key: &str) {
    unsafe { std::env::remove_var(key) }
}

fn write_test_manifest(root: &Path) {
    let manifest = json!({
        "id": "dev.test",
        "name": "Dev Test",
        "version": "0.1.0",
        "world": "greentic:component/component@0.4.0",
        "describe_export": "get-manifest",
        "supports": ["messaging"],
        "profiles": { "default": "dev", "supported": ["dev"] },
        "capabilities": { "wasi": {}, "host": {} },
        "artifacts": { "component_wasm": "component.wasm" },
        "hashes": { "component_wasm": "blake3:0" },
        "config_schema": {},
        "dev_flows": {
            "default": {
                "format": "flow-ir-json",
                "graph": {
                    "schema_version": 1,
                    "id": "component.default",
                    "type": "component-config",
                    "nodes": {
                        "emit_config": {
                            "template": "{ \"node_id\": \"qa_step\", \"node\": { \"qa\": { \"component\": \"component-qa-process\", \"question\": \"hi\" }, \"routing\": [{ \"to\": \"NEXT_NODE_PLACEHOLDER\" }] } }"
                        }
                    },
                    "edges": []
                }
            }
        }
    });
    fs::write(
        root.join("component.manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

fn write_manifest_missing_type(root: &Path) -> JsonValue {
    let graph = json!({
        "schema_version": 1,
        "id": "component.default",
        "nodes": {
            "emit_config": {
                "template": "{ \"node_id\": \"qa_step\", \"node\": { \"qa\": { \"component\": \"component-qa-process\", \"question\": \"hi\" }, \"routing\": [{ \"to\": \"NEXT_NODE_PLACEHOLDER\" }] } }"
            }
        },
        "edges": []
    });
    let manifest = json!({
        "id": "dev.test",
        "name": "Dev Test",
        "version": "0.1.0",
        "world": "greentic:component/component@0.4.0",
        "describe_export": "get-manifest",
        "supports": ["messaging"],
        "profiles": { "default": "dev", "supported": ["dev"] },
        "capabilities": { "wasi": {}, "host": {} },
        "artifacts": { "component_wasm": "component.wasm" },
        "hashes": { "component_wasm": "blake3:0" },
        "config_schema": {},
        "dev_flows": {
            "default": {
                "format": "flow-ir-json",
                "graph": graph
            }
        }
    });
    fs::write(
        root.join("component.manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    graph
}

fn write_pack_flow(root: &Path) -> PathBuf {
    let flows = root.join("flows");
    fs::create_dir_all(&flows).unwrap();
    let flow = "schema_version: 1
id: pack.demo
type: pack
nodes:
  start:
    routing:
      - to: end
  end: {}
";
    let path = flows.join("demo.ygtc");
    fs::write(&path, flow).unwrap();
    path
}

#[test]
fn parse_config_flow_rejects_invalid() {
    let bad = r#"{"node": {"qa":{} } }"#;
    let err = parse_config_flow_output(bad).expect_err("missing node_id should error");
    assert!(
        err.to_string().contains("node_id"),
        "expected node_id error"
    );
}

#[test]
fn render_config_flow_adds_missing_type() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let graph = write_manifest_missing_type(&root);

    let yaml = render_config_flow_yaml("default", &graph).expect("render config flow yaml");
    let parsed: JsonValue = serde_yaml::from_str(&yaml).expect("parse rendered yaml");
    assert_eq!(
        parsed.get("type").and_then(|v| v.as_str()),
        Some("component-config"),
        "rendered config flow should include default type"
    );
    load_and_validate_bundle(&yaml, None)
        .expect("rendered config flow should satisfy schema validation");
}

fn write_component_bundle(tmp: &Path) -> PathBuf {
    let bundle = tmp.join("component-bundle");
    let flows = bundle.join("flows");
    fs::create_dir_all(&flows).unwrap();
    let default = "schema_version: 1
id: component.default
type: component-config
nodes:
  emit_config:
    template: |
      {
        \"node_id\": \"qa_step\",
        \"node\": {
          \"qa\": {
            \"component\": \"component-qa-process\",
            \"question\": \"hi\"
          },
          \"routing\": [
            { \"to\": \"NEXT_NODE_PLACEHOLDER\" }
          ]
        }
      }
";
    fs::write(flows.join("default.ygtc"), default).unwrap();
    bundle
}

fn assert_qa_step_inserted(root: &Path) {
    let updated = fs::read_to_string(root.join("flows/demo.ygtc")).unwrap();
    let doc: serde_yaml::Value = serde_yaml::from_str(&updated).unwrap();
    let tagless = |s: &str| serde_yaml::Value::String(s.to_string(), None);
    let nodes = doc
        .get(&tagless("nodes"))
        .and_then(|n| n.as_mapping())
        .expect("nodes map");
    assert!(nodes.get(&tagless("qa_step")).is_some());
    let routing = nodes
        .get(&tagless("start"))
        .and_then(|node| node.as_mapping().and_then(|m| m.get(&tagless("routing"))))
        .and_then(|r| r.as_sequence())
        .expect("routing array");
    assert!(
        routing.iter().any(|entry| entry
            .as_mapping()
            .and_then(|m| m.get(&tagless("to")))
            .and_then(|v| v.as_str())
            .map(|s| s == "qa_step")
            .unwrap_or(false)),
        "expected routing to include qa_step"
    );
}

#[test]
fn flow_add_step_inserts_node() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    write_test_manifest(&root);
    let bundle = write_component_bundle(&root);
    write_pack_flow(&root);

    let _guard = WORKDIR_LOCK.lock().unwrap();
    let prev_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&root).unwrap();

    run_add_step(FlowAddStepArgs {
        flow_id: "demo".into(),
        coordinate: Some(bundle.to_string_lossy().to_string()),
        profile: None,
        mode: Some(ConfigFlowModeArg::Default),
        after: Some("start".into()),
        flow: "default".into(),
        manifest: None,
    })
    .unwrap();
    std::env::set_current_dir(prev_dir).unwrap();

    assert_qa_step_inserted(&root);
}

#[test]
fn flow_add_step_recovers_missing_type() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let graph = write_manifest_missing_type(&root);
    let bundle = write_component_bundle(&root);
    write_pack_flow(&root);

    // Sanity-check the rendered YAML validates when `type` is missing in manifest.
    let rendered =
        render_config_flow_yaml("default", &graph).expect("render config flow from manifest");
    load_and_validate_bundle(&rendered, None).expect("rendered flow should validate");

    let _guard = WORKDIR_LOCK.lock().unwrap();
    let prev_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&root).unwrap();

    run_add_step(FlowAddStepArgs {
        flow_id: "demo".into(),
        coordinate: Some(bundle.to_string_lossy().to_string()),
        profile: None,
        mode: Some(ConfigFlowModeArg::Default),
        after: Some("start".into()),
        flow: "default".into(),
        manifest: None,
    })
    .expect("add-step should tolerate missing type");
    std::env::set_current_dir(prev_dir).unwrap();

    assert_qa_step_inserted(&root);
}

#[test]
fn flow_add_step_errors_when_config_flow_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let manifest = json!({
        "id": "dev.test",
        "name": "Dev Test",
        "version": "0.1.0",
        "world": "greentic:component/component@0.4.0",
        "describe_export": "get-manifest",
        "supports": ["messaging"],
        "profiles": { "default": "dev", "supported": ["dev"] },
        "capabilities": { "wasi": {}, "host": {} },
        "artifacts": { "component_wasm": "component.wasm" },
        "hashes": { "component_wasm": "blake3:0" },
        "config_schema": {}
    });
    fs::write(
        root.join("component.manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let manifest_struct: greentic_types::component::ComponentManifest =
        serde_json::from_value(manifest).unwrap();
    assert!(
        manifest_struct.dev_flows.is_empty(),
        "expected manifest to lack dev_flows for error test"
    );
    write_pack_flow(&root);

    let _guard = WORKDIR_LOCK.lock().unwrap();
    let prev_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&root).unwrap();
    let err = run_add_step(FlowAddStepArgs {
        flow_id: "demo".into(),
        coordinate: Some(root.to_string_lossy().to_string()),
        profile: None,
        mode: None,
        after: Some("start".into()),
        flow: "default".into(),
        manifest: None,
    })
    .expect_err("expected missing config flow error");
    std::env::set_current_dir(prev_dir).unwrap();
    assert!(
        err.to_string().contains("Flow 'default' is missing"),
        "unexpected error: {err}"
    );
}

#[test]
fn flow_add_step_errors_when_pack_flow_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    write_test_manifest(&root);
    let bundle = write_component_bundle(&root);

    let _guard = WORKDIR_LOCK.lock().unwrap();
    let prev_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&root).unwrap();
    let err = run_add_step(FlowAddStepArgs {
        flow_id: "missing".into(),
        coordinate: Some(bundle.to_string_lossy().to_string()),
        profile: None,
        mode: None,
        after: None,
        flow: "default".into(),
        manifest: None,
    })
    .expect_err("expected missing pack flow error");
    std::env::set_current_dir(prev_dir).unwrap();
    assert!(
        err.to_string().contains("Pack flow 'missing'"),
        "unexpected error: {err}"
    );
}

#[test]
fn flow_add_step_respects_offline_without_stub() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    write_test_manifest(&root);
    write_pack_flow(&root);

    let _guard = WORKDIR_LOCK.lock().unwrap();
    let prev_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&root).unwrap();

    let prev_offline = std::env::var("GREENTIC_DEV_OFFLINE").ok();
    let prev_stub = std::env::var("GREENTIC_DEV_RESOLVE_STUB").ok();
    set_env("GREENTIC_DEV_OFFLINE", "1");
    remove_env("GREENTIC_DEV_RESOLVE_STUB");

    let err = run_add_step(FlowAddStepArgs {
        flow_id: "demo".into(),
        coordinate: Some("component://greentic/example@^1".into()),
        profile: None,
        mode: Some(ConfigFlowModeArg::Default),
        after: Some("start".into()),
        flow: "default".into(),
        manifest: None,
    })
    .expect_err("offline add-step should reject remote coordinate without stub");

    if let Some(val) = prev_offline {
        set_env("GREENTIC_DEV_OFFLINE", &val);
    } else {
        remove_env("GREENTIC_DEV_OFFLINE");
    }
    if let Some(val) = prev_stub {
        set_env("GREENTIC_DEV_RESOLVE_STUB", &val);
    } else {
        remove_env("GREENTIC_DEV_RESOLVE_STUB");
    }
    std::env::set_current_dir(prev_dir).unwrap();

    assert!(
        err.to_string().contains("offline mode enabled"),
        "unexpected error: {err}"
    );
}

#[test]
fn flow_add_step_uses_stubbed_resolve() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    write_test_manifest(&root);
    write_pack_flow(&root);

    // Create a local artifact file referenced by the stub.
    let artifact_path = root.join("artifact.wasm");
    fs::write(&artifact_path, b"00").unwrap();

    let stub = json!({
        "artifact_path": artifact_path.display().to_string(),
        "digest": "sha256:stub"
    });
    let stub_path = root.join("stub.json");
    fs::write(&stub_path, serde_json::to_string(&stub).unwrap()).unwrap();

    let _guard = WORKDIR_LOCK.lock().unwrap();
    let prev_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&root).unwrap();

    let prev_offline = std::env::var("GREENTIC_DEV_OFFLINE").ok();
    let prev_stub = std::env::var("GREENTIC_DEV_RESOLVE_STUB").ok();
    set_env("GREENTIC_DEV_OFFLINE", "1");
    set_env(
        "GREENTIC_DEV_RESOLVE_STUB",
        stub_path.to_string_lossy().as_ref(),
    );

    run_add_step(FlowAddStepArgs {
        flow_id: "demo".into(),
        coordinate: Some("component://greentic/example@^1".into()),
        profile: None,
        mode: Some(ConfigFlowModeArg::Default),
        after: Some("start".into()),
        flow: "default".into(),
        manifest: None,
    })
    .expect("stubbed resolve should allow offline add-step");

    if let Some(val) = prev_offline {
        set_env("GREENTIC_DEV_OFFLINE", &val);
    } else {
        remove_env("GREENTIC_DEV_OFFLINE");
    }
    if let Some(val) = prev_stub {
        set_env("GREENTIC_DEV_RESOLVE_STUB", &val);
    } else {
        remove_env("GREENTIC_DEV_RESOLVE_STUB");
    }
    std::env::set_current_dir(prev_dir).unwrap();

    assert_qa_step_inserted(&root);
}
