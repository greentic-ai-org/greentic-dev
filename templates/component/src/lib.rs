use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

wit_bindgen::generate!({
    path: "wit",
    world: "component",
    generate_all,
});

struct Component;

export!(Component);

#[derive(Debug, Deserialize)]
struct InvokeInput {
    #[serde(default)]
    message: String,
}

#[derive(Debug, Serialize)]
struct InvokeOutput {
    echoed: String,
    operation: String,
}

impl exports::greentic::component::node::Guest for Component {
    fn get_manifest() -> String {
        let schema: Value = serde_json::from_str(include_str!("../schemas/v1/config.schema.json"))
            .expect("config schema json to be valid");
        json!({
            "name": "{{component_name}}",
            "description": "Generated Greentic component scaffold",
            "capabilities": ["telemetry"],
            "exports": [
                { "operation": "invoke" }
            ],
            "config_schema": schema,
            "secrets": [],
            "wit_compat": {
                "package": "greentic:component",
                "min": "{{component_world_version}}",
                "max": "{{component_world_version}}"
            }
        })
        .to_string()
    }

    fn on_start(
        _ctx: exports::greentic::component::node::ExecCtx,
    ) -> Result<exports::greentic::component::node::LifecycleStatus, String> {
        Ok(exports::greentic::component::node::LifecycleStatus::Ok)
    }

    fn on_stop(
        _ctx: exports::greentic::component::node::ExecCtx,
        _reason: String,
    ) -> Result<exports::greentic::component::node::LifecycleStatus, String> {
        Ok(exports::greentic::component::node::LifecycleStatus::Ok)
    }

    fn invoke(
        _ctx: exports::greentic::component::node::ExecCtx,
        operation: String,
        input: String,
    ) -> exports::greentic::component::node::InvokeResult {
        let payload: InvokeInput =
            serde_json::from_str(&input).unwrap_or(InvokeInput { message: input });
        let response = InvokeOutput {
            echoed: payload.message,
            operation,
        };
        exports::greentic::component::node::InvokeResult::Ok(
            serde_json::to_string(&response).expect("serialize response"),
        )
    }

    fn invoke_stream(
        _ctx: exports::greentic::component::node::ExecCtx,
        _operation: String,
        _input: String,
    ) -> Vec<exports::greentic::component::node::StreamEvent> {
        Vec::new()
    }
}
