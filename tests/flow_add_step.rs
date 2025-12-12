use std::fs;
use std::path::{Path, PathBuf};

use greentic_dev::cli::{ConfigFlowModeArg, FlowAddStepArgs};
use greentic_dev::flow_cmd::parse_config_flow_output;
use greentic_dev::flow_cmd::run_add_step;
use serde_json::json;

fn write_test_flow(root: &Path) {
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
            "demo": {
                "format": "flow-ir-json",
                "graph": {
                    "nodes": {
                        "start": { "routing": [] }
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

#[test]
fn parse_config_flow_rejects_invalid() {
    let bad = r#"{"node": {"qa":{} } }"#;
    let err = parse_config_flow_output(bad).expect_err("missing node_id should error");
    assert!(
        err.to_string().contains("node_id"),
        "expected node_id error"
    );
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

#[test]
fn flow_add_step_inserts_node() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    write_test_flow(&root);
    let bundle = write_component_bundle(&root);

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

    let updated = fs::read_to_string(root.join("component.manifest.json")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&updated).unwrap();
    let nodes = doc
        .get("dev_flows")
        .and_then(|f| f.get("demo"))
        .and_then(|f| f.get("graph"))
        .and_then(|f| f.get("nodes"))
        .and_then(|n| n.as_object())
        .expect("nodes map");
    assert!(
        nodes.contains_key("qa_step"),
        "expected new node to be inserted"
    );
    let routing = nodes
        .get("start")
        .and_then(|node| node.get("routing"))
        .and_then(|r| r.as_array())
        .expect("routing array");
    assert!(
        routing.iter().any(|entry| entry
            .get("to")
            .and_then(|v| v.as_str())
            .map(|s| s == "qa_step")
            .unwrap_or(false)),
        "expected routing to include qa_step"
    );
}
