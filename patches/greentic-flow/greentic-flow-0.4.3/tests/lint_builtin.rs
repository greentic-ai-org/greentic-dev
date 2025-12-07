use greentic_flow::{lint::lint_builtin_rules, loader::load_ygtc_from_str, to_ir};
use std::path::Path;

#[test]
fn lint_flags_missing_start_node() {
    let yaml = r#"
id: demo
type: messaging
start: missing
nodes:
  entry:
    qa.process: {}
"#;
    let flow = load_ygtc_from_str(yaml, Path::new("schemas/ygtc.flow.schema.json")).unwrap();
    let ir = to_ir(flow).unwrap();
    let errors = lint_builtin_rules(&ir);
    assert!(
        errors.iter().any(|e| e.contains("start node 'missing'")),
        "expected missing start node lint, got {errors:?}"
    );
}

#[test]
fn lint_passes_when_start_node_exists() {
    let yaml = r#"
id: demo
type: messaging
start: entry
nodes:
  entry:
    qa.process: {}
"#;
    let flow = load_ygtc_from_str(yaml, Path::new("schemas/ygtc.flow.schema.json")).unwrap();
    let ir = to_ir(flow).unwrap();
    let errors = lint_builtin_rules(&ir);
    assert!(errors.is_empty(), "unexpected lint errors: {errors:?}");
}
