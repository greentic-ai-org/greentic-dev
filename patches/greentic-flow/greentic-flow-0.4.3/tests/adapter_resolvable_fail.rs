use greentic_flow::{
    lint::lint_with_registry, loader::load_ygtc_from_str, registry::AdapterCatalog, to_ir,
};
use std::path::Path;

#[test]
fn adapter_nodes_fail_with_missing_operations() {
    let schema = Path::new("schemas/ygtc.flow.schema.json");
    let yaml = std::fs::read_to_string("tests/data/flow_fail.ygtc").unwrap();
    let flow = load_ygtc_from_str(&yaml, schema).unwrap();
    let ir = to_ir(flow).unwrap();
    let catalog = AdapterCatalog::load_from_file("tests/data/registry_ok.json").unwrap();

    let errors = lint_with_registry(&ir, &catalog);

    assert_eq!(errors.len(), 2, "expected exactly two lint errors");
    assert!(
        errors
            .iter()
            .any(|e| e.contains("messaging.telegram.deleteUniverse"))
    );
    assert!(errors.iter().any(|e| e.contains("email.google.beamMeUp")));
}
