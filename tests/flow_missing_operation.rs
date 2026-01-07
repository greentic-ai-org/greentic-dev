use greentic_dev::cli::{ConfigFlowModeArg, FlowAddStepArgs};
use greentic_dev::flow_cmd::run_add_step;
use serde_json::json;
use std::fs;
use std::path::Path;

fn write_manifest_without_ops(root: &Path) {
    let manifest = json!({
        "id": "dev.hello",
        "name": "Hello",
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
                            "template": "{ \"node_id\": \"hello\", \"node\": { \"tool\": { \"component\": \"ai.greentic.hello\" }, \"routing\": [{ \"to\": \"NEXT_NODE_PLACEHOLDER\" }] } }"
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

fn write_pack_flow(root: &Path) {
    let flow = "schema_version: 1
id: main
type: messaging
start: start
nodes:
  start:
    dev.greentic.qa:
      question: \"hello?\"
    routing:
      - to: end
  end:
    dev.greentic.qa:
      question: \"done\"
";
    fs::create_dir_all(root.join("flows")).unwrap();
    fs::write(root.join("flows/main.ygtc"), flow).unwrap();
}

#[test]
fn add_step_errors_when_config_flow_has_tool_without_ops() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    write_manifest_without_ops(&root);
    write_pack_flow(&root);

    let prev = std::env::current_dir().unwrap();
    std::env::set_current_dir(&root).unwrap();

    let err = run_add_step(FlowAddStepArgs {
        flow_id: "main".into(),
        coordinate: Some(root.to_string_lossy().to_string()),
        profile: None,
        mode: Some(ConfigFlowModeArg::Default),
        after: Some("start".into()),
        flow: "default".into(),
        manifest: Some("component.manifest.json".into()),
    })
    .expect_err("add-step should error when config flow emits tool without operations");

    std::env::set_current_dir(prev).unwrap();
    let msg = err.to_string();
    assert!(
        msg.contains("ADD_STEP_NODE_INVALID") || msg.contains("Legacy tool emission"),
        "missing config flow hint: {msg}"
    );
    assert!(
        msg.contains("Legacy tool emission is not supported"),
        "missing tool emission hint: {msg}"
    );
}
