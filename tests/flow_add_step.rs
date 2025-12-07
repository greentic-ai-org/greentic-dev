use std::fs;
use std::path::PathBuf;

use greentic_dev::cli::{ConfigFlowModeArg, FlowAddStepArgs};
use greentic_dev::flow_cmd::parse_config_flow_output;
use greentic_dev::flow_cmd::run_add_step;

fn write_test_flow(root: &PathBuf) {
    let flows = root.join("flows");
    fs::create_dir_all(&flows).unwrap();
    let flow = "schema_version: 1
id: demo
type: component-config
nodes:
  start:
    template: \"{}\"
";
    fs::write(flows.join("demo.ygtc"), flow).unwrap();
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

fn write_component_bundle(tmp: &PathBuf) -> PathBuf {
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
    })
    .unwrap();

    let updated =
        fs::read_to_string(root.join("flows").join("demo.ygtc")).expect("flow should exist");
    assert!(
        updated.contains("qa_step"),
        "expected new node to be inserted"
    );
    assert!(
        updated.contains("to: start"),
        "expected routing to be updated"
    );
}
