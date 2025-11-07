use greentic_dev::dev_runner;

#[test]
fn schema_validation_passes_for_examples() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/flows/min.ygtc");
    let yaml = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    let doc: serde_yaml_bw::Value = serde_yaml_bw::from_str(&yaml).unwrap();
    let schema = r#"{"type":"object"}"#; // placeholder schema
    assert!(dev_runner::schema::validate_yaml_against_schema(&doc, schema).is_ok());
}
