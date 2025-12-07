use greentic_flow::{config_flow::run_config_flow, loader::load_ygtc_from_str, to_ir};
use serde_json::{Map, Value, json};
use std::path::Path;

#[test]
fn config_flow_loads_and_emits_contract_payload() {
    let yaml = std::fs::read_to_string("tests/data/config_flow.ygtc").unwrap();
    let flow = load_ygtc_from_str(&yaml, Path::new("schemas/ygtc.flow.schema.json")).unwrap();
    assert_eq!(flow.flow_type, "component-config");

    let ir = to_ir(flow).unwrap();
    let ask = ir.nodes.get("ask_config").expect("ask_config node");
    assert_eq!(ask.component, "questions");
    assert!(
        ask.payload_expr
            .pointer("/fields")
            .and_then(Value::as_array)
            .map(|fields| !fields.is_empty())
            .unwrap_or(false)
    );

    let emit = ir.nodes.get("emit_config").expect("emit_config node");
    assert_eq!(emit.component, "template");
    let template_str = emit
        .payload_expr
        .as_str()
        .expect("template payload is a string");
    let rendered: Value =
        serde_json::from_str(template_str).expect("template payload should be valid JSON");
    let node_id = rendered
        .get("node_id")
        .and_then(Value::as_str)
        .expect("node_id present");
    assert_eq!(node_id, "qa_step");
    let node = rendered
        .get("node")
        .and_then(Value::as_object)
        .expect("node object");
    assert!(node.contains_key("qa"));
}

#[test]
fn config_flow_harness_substitutes_state() {
    let yaml = std::fs::read_to_string("tests/data/config_flow.ygtc").unwrap();
    let mut answers = Map::new();
    answers.insert("welcome_template".to_string(), json!("Howdy"));
    answers.insert("temperature".to_string(), json!(0.5));

    let output =
        run_config_flow(&yaml, Path::new("schemas/ygtc.flow.schema.json"), &answers).unwrap();

    assert_eq!(output.node_id, "qa_step");
    let qa = output.node.get("qa").and_then(Value::as_object).unwrap();
    assert_eq!(qa.get("welcome_template"), Some(&json!("Howdy")));
    assert_eq!(qa.get("temperature"), Some(&json!(0.5)));
}
