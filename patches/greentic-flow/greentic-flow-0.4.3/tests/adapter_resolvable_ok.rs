use greentic_flow::{
    lint::lint_with_registry, loader::load_ygtc_from_str, registry::AdapterCatalog, to_ir,
};
use std::path::Path;

#[test]
fn adapter_nodes_resolve_with_registry() {
    let schema = Path::new("schemas/ygtc.flow.schema.json");
    let yaml = std::fs::read_to_string("tests/data/flow_ok.ygtc").unwrap();
    let flow = load_ygtc_from_str(&yaml, schema).unwrap();
    let ir = to_ir(flow).unwrap();
    let catalog = AdapterCatalog::load_from_file("tests/data/registry_ok.json").unwrap();

    let errors = lint_with_registry(&ir, &catalog);

    assert!(errors.is_empty(), "expected no lint errors, got {errors:?}");
}
