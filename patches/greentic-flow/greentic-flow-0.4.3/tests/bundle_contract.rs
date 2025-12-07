use greentic_flow::{flow_bundle::load_and_validate_bundle, lint_to_stdout_json};
use serde_json::Value;

#[test]
fn bundle_hash_matches_cli_output() {
    let yaml = std::fs::read_to_string("fixtures/flow_ok.ygtc").unwrap();
    let bundle = load_and_validate_bundle(&yaml, None).unwrap();

    let cli_json = std::fs::read_to_string("fixtures/flow_ok.bundle.json").unwrap();
    let parsed: Value = serde_json::from_str(&cli_json).unwrap();
    assert!(parsed["ok"].as_bool().unwrap());
    let cli_hash = parsed["hash_blake3"].as_str().unwrap();
    assert_eq!(bundle.hash_blake3, cli_hash);

    let helper_json = lint_to_stdout_json(&yaml);
    let helper: Value = serde_json::from_str(&helper_json).unwrap();
    assert_eq!(helper, parsed, "helper output must match CLI contract");
}
