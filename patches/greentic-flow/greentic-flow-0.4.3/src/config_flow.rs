use std::path::Path;

use lazy_static::lazy_static;
use regex::Regex;
use serde_json::{Map, Value};

use crate::{
    error::{FlowError, FlowErrorLocation, Result},
    loader::load_ygtc_from_str,
    to_ir,
};

/// Result of executing a config flow: a node identifier and the node object to insert.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigFlowOutput {
    pub node_id: String,
    pub node: Value,
}

/// Execute a minimal, single-pass config-flow harness.
///
/// Supported components:
/// - `questions`: seeds state values from provided answers or defaults.
/// - `template`: renders the template payload, replacing `{{state.key}}` placeholders inside strings.
///
/// The flow ends when a `template` node is executed. Routing follows the first non-out route if
/// present, otherwise stops.
pub fn run_config_flow(
    yaml: &str,
    schema_path: &Path,
    answers: &Map<String, Value>,
) -> Result<ConfigFlowOutput> {
    let flow = load_ygtc_from_str(yaml, schema_path)?;
    let ir = to_ir(flow)?;

    let mut state = answers.clone();

    let mut current = resolve_entry(&ir);
    let mut visited = 0usize;
    while visited < ir.nodes.len().saturating_add(4) {
        visited += 1;
        let node = ir.nodes.get(&current).ok_or_else(|| FlowError::Internal {
            message: format!("node '{current}' missing during config flow execution"),
            location: FlowErrorLocation::at_path(format!("nodes.{current}")),
        })?;

        match node.component.as_str() {
            "questions" => {
                apply_questions(&node.payload_expr, &mut state)?;
            }
            "template" => {
                let payload = render_template(&node.payload_expr, &state)?;
                return extract_config_output(payload);
            }
            other => {
                return Err(FlowError::Internal {
                    message: format!("unsupported component '{other}' in config flow"),
                    location: FlowErrorLocation::at_path(format!("nodes.{current}")),
                });
            }
        }

        // Move to the next routed node if available.
        let mut next = None;
        for route in &node.routes {
            if let Some(to) = &route.to {
                next = Some(to.clone());
                break;
            }
        }
        match next {
            Some(id) => current = id,
            None => {
                return Err(FlowError::Internal {
                    message: "config flow terminated without reaching template node".to_string(),
                    location: FlowErrorLocation::at_path("nodes".to_string()),
                });
            }
        }
    }

    Err(FlowError::Internal {
        message: "config flow exceeded traversal limit".to_string(),
        location: FlowErrorLocation::at_path("nodes".to_string()),
    })
}

fn resolve_entry(ir: &crate::ir::FlowIR) -> String {
    if let Some(start) = &ir.start {
        return start.clone();
    }
    if ir.nodes.contains_key("in") {
        return "in".to_string();
    }
    ir.nodes
        .keys()
        .next()
        .cloned()
        .unwrap_or_else(|| "in".to_string())
}

fn apply_questions(payload: &Value, state: &mut Map<String, Value>) -> Result<()> {
    let fields = payload
        .get("fields")
        .and_then(Value::as_array)
        .ok_or_else(|| FlowError::Internal {
            message: "questions node missing fields array".to_string(),
            location: FlowErrorLocation::at_path("questions.fields".to_string()),
        })?;

    for field in fields {
        let id = field
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| FlowError::Internal {
                message: "questions field missing id".to_string(),
                location: FlowErrorLocation::at_path("questions.fields".to_string()),
            })?;
        if state.contains_key(id) {
            continue;
        }
        if let Some(default) = field.get("default") {
            state.insert(id.to_string(), default.clone());
        } else {
            return Err(FlowError::Internal {
                message: format!("missing answer for '{id}'"),
                location: FlowErrorLocation::at_path(format!("questions.fields.{id}")),
            });
        }
    }
    Ok(())
}

fn render_template(payload: &Value, state: &Map<String, Value>) -> Result<Value> {
    let template_str = payload.as_str().ok_or_else(|| FlowError::Internal {
        message: "template node payload must be a string".to_string(),
        location: FlowErrorLocation::at_path("template".to_string()),
    })?;
    let mut value: Value = serde_json::from_str(template_str).map_err(|e| FlowError::Internal {
        message: format!("template JSON parse error: {e}"),
        location: FlowErrorLocation::at_path("template".to_string()),
    })?;
    substitute_state(&mut value, state)?;
    Ok(value)
}

lazy_static! {
    static ref STATE_RE: Regex = Regex::new(r"^\{\{\s*state\.([A-Za-z_]\w*)\s*\}\}$").unwrap();
}

fn substitute_state(target: &mut Value, state: &Map<String, Value>) -> Result<()> {
    match target {
        Value::String(s) => {
            if let Some(caps) = STATE_RE.captures(s) {
                let key = caps.get(1).unwrap().as_str();
                let val = state.get(key).ok_or_else(|| FlowError::Internal {
                    message: format!("state value for '{key}' not found"),
                    location: FlowErrorLocation::at_path(format!("state.{key}")),
                })?;
                *target = val.clone();
            }
            Ok(())
        }
        Value::Array(items) => {
            for item in items {
                substitute_state(item, state)?;
            }
            Ok(())
        }
        Value::Object(map) => {
            for value in map.values_mut() {
                substitute_state(value, state)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn extract_config_output(value: Value) -> Result<ConfigFlowOutput> {
    let node_id = value
        .get("node_id")
        .and_then(Value::as_str)
        .ok_or_else(|| FlowError::Internal {
            message: "config flow output missing node_id".to_string(),
            location: FlowErrorLocation::at_path("node_id".to_string()),
        })?
        .to_string();
    let node = value
        .get("node")
        .cloned()
        .ok_or_else(|| FlowError::Internal {
            message: "config flow output missing node".to_string(),
            location: FlowErrorLocation::at_path("node".to_string()),
        })?;
    Ok(ConfigFlowOutput { node_id, node })
}
