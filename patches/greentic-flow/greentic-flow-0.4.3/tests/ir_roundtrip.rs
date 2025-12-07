use greentic_flow::{loader::load_ygtc_from_str, to_ir};
use std::path::Path;

#[test]
fn ir_serializes() {
    let yaml = std::fs::read_to_string("fixtures/weather_bot.ygtc").unwrap();
    let flow = load_ygtc_from_str(&yaml, Path::new("schemas/ygtc.flow.schema.json")).unwrap();
    let ir = to_ir(flow).unwrap();
    let json = serde_json::to_string(&ir).unwrap();
    assert!(json.contains("\"component\":\"qa.process\""));
}
