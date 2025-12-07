use assert_cmd::Command;
use serde_json::Value;

#[test]
fn json_mode_emits_bundle() {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ygtc-lint"));
    let assert = cmd
        .arg("--json")
        .arg("fixtures/weather_bot.ygtc")
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let payload: Value = serde_json::from_str(&stdout).unwrap();
    assert!(payload["ok"].as_bool().unwrap());
    assert_eq!(payload["bundle"]["id"].as_str(), Some("weather_bot"));
    assert!(payload["hash_blake3"].as_str().is_some());
}

#[test]
fn json_mode_reports_schema_errors() {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ygtc-lint"));
    let assert = cmd
        .arg("--json")
        .arg("tests/data/flow_missing_type.ygtc")
        .assert()
        .failure();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let payload: Value = serde_json::from_str(&stdout).unwrap();
    assert!(!payload["ok"].as_bool().unwrap());
    let errors = payload["errors"].as_array().expect("errors array");
    assert!(
        errors
            .iter()
            .any(|error| error.get("json_pointer").and_then(Value::as_str).is_some()),
        "expected an error with a json_pointer"
    );
}

#[test]
fn json_mode_stdin_reports_schema_pointer() {
    let stdin_flow = std::fs::read_to_string("tests/data/flow_missing_type.ygtc").unwrap();
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ygtc-lint"));
    let assert = cmd
        .arg("--json")
        .arg("--stdin")
        .write_stdin(stdin_flow)
        .assert()
        .failure();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let payload: Value = serde_json::from_str(&stdout).unwrap();
    assert!(!payload["ok"].as_bool().unwrap());
    let errors = payload["errors"].as_array().expect("errors array");
    assert!(
        errors
            .iter()
            .any(|error| error.get("json_pointer").and_then(Value::as_str).is_some()),
        "expected an error with a json_pointer"
    );
}
