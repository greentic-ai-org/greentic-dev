use crate::error::{FlowError, FlowErrorLocation, Result};
use lazy_static::lazy_static;
use regex::Regex;
use serde_json::Value;

lazy_static! {
    static ref REF_RE: Regex = Regex::new(r"^[a-zA-Z_]\w*(?:\.[a-zA-Z_]\w*)+$").unwrap();
}

/// Resolve only `parameters.*` references recursively in a JSON value.
pub fn resolve_parameters(value: &Value, parameters: &Value, loc: &str) -> Result<Value> {
    match value {
        Value::String(s) if REF_RE.is_match(s) => {
            if let Some(rest) = s.strip_prefix("parameters.") {
                return lookup(parameters, rest, loc);
            }
            Ok(Value::String(s.clone()))
        }
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (idx, item) in items.iter().enumerate() {
                out.push(resolve_parameters(
                    item,
                    parameters,
                    &format!("{loc}[{idx}]"),
                )?);
            }
            Ok(Value::Array(out))
        }
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, item) in map {
                out.insert(
                    key.clone(),
                    resolve_parameters(item, parameters, &format!("{loc}.{key}"))?,
                );
            }
            Ok(Value::Object(out))
        }
        _ => Ok(value.clone()),
    }
}

fn lookup(root: &Value, path: &str, loc: &str) -> Result<Value> {
    let mut current = root;
    for part in path.split('.') {
        current = match current {
            Value::Object(map) => map.get(part).ok_or_else(|| FlowError::Internal {
                message: format!("Unknown parameters.{path} at {loc}"),
                location: FlowErrorLocation::at_path(loc.to_string()),
            })?,
            _ => {
                return Err(FlowError::Internal {
                    message: format!("parameters.{path} at {loc} not an object"),
                    location: FlowErrorLocation::at_path(loc.to_string()),
                });
            }
        };
    }
    Ok(current.clone())
}
