use greentic_flow::{loader::load_ygtc_from_str, resolve::resolve_parameters, to_ir};
use serde_json::json;
use std::path::Path;

#[test]
fn load_weather_ir_and_resolve_params() {
    let yaml = std::fs::read_to_string("fixtures/weather_bot.ygtc").unwrap();
    let flow = load_ygtc_from_str(&yaml, Path::new("schemas/ygtc.flow.schema.json")).unwrap();
    let ir = to_ir(flow).unwrap();

    assert_eq!(ir.id, "weather_bot");
    assert_eq!(ir.flow_type, "messaging");
    assert_eq!(ir.start.as_deref(), Some("in"));

    let fw = ir.nodes.get("forecast_weather").unwrap();
    assert_eq!(fw.component, "mcp.exec");

    let resolved =
        resolve_parameters(&fw.payload_expr, &ir.parameters, "nodes.forecast_weather").unwrap();
    assert_eq!(resolved.pointer("/args/days").unwrap(), &json!(3));
    assert_eq!(
        resolved.pointer("/args/q").unwrap(),
        &json!("in.q_location")
    );
}
