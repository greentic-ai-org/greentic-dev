use crate::{
    error::{FlowError, FlowErrorLocation, Result, SchemaErrorDetail},
    model::FlowDoc,
    path_safety::normalize_under_root,
    util::is_valid_component_key,
};
use jsonschema::Draft;
use serde_json::Value;
use serde_yaml_bw::Location as YamlLocation;
use std::{collections::BTreeMap, path::Path};

const INLINE_SOURCE: &str = "<inline>";

fn validate_json(
    doc: &Value,
    schema_text: &str,
    schema_label: &str,
    schema_path: Option<&Path>,
    source_label: &str,
    source_path: Option<&Path>,
) -> Result<()> {
    let schema: Value = serde_json::from_str(schema_text).map_err(|e| FlowError::Internal {
        message: format!("schema parse for {schema_label}: {e}"),
        location: FlowErrorLocation::at_path(schema_label.to_string())
            .with_source_path(schema_path),
    })?;
    let validator = jsonschema::options()
        .with_draft(Draft::Draft202012)
        .build(&schema)
        .map_err(|e| FlowError::Internal {
            message: format!("schema compile for {schema_label}: {e}"),
            location: FlowErrorLocation::at_path(schema_label.to_string())
                .with_source_path(schema_path),
        })?;
    let details: Vec<SchemaErrorDetail> = validator
        .iter_errors(doc)
        .map(|e| {
            let pointer = e.instance_path().to_string();
            let pointer = if pointer.is_empty() {
                "/".to_string()
            } else {
                pointer
            };
            SchemaErrorDetail {
                message: e.to_string(),
                location: FlowErrorLocation::at_path(format!("{source_label}{pointer}"))
                    .with_source_path(source_path)
                    .with_json_pointer(Some(pointer.clone())),
            }
        })
        .collect();
    if !details.is_empty() {
        let message = details
            .iter()
            .map(|detail| {
                let where_str = detail
                    .location
                    .describe()
                    .unwrap_or_else(|| source_label.to_string());
                format!("{where_str}: {}", detail.message)
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Err(FlowError::Schema {
            message,
            details,
            location: FlowErrorLocation::at_path(source_label.to_string())
                .with_source_path(source_path),
        });
    }
    Ok(())
}

pub fn load_ygtc_from_str(yaml: &str, schema_path: &Path) -> Result<FlowDoc> {
    load_ygtc_from_str_with_source(yaml, schema_path, INLINE_SOURCE)
}

pub fn load_ygtc_from_str_with_source(
    yaml: &str,
    schema_path: &Path,
    source_label: impl Into<String>,
) -> Result<FlowDoc> {
    let schema_root = std::env::current_dir().map_err(|e| FlowError::Internal {
        message: format!("resolve schema root: {e}"),
        location: FlowErrorLocation::at_path(schema_path.display().to_string())
            .with_source_path(Some(schema_path)),
    })?;
    let safe_schema_path =
        normalize_under_root(&schema_root, schema_path).map_err(|e| FlowError::Internal {
            message: format!("schema path validation for {}: {e}", schema_path.display()),
            location: FlowErrorLocation::at_path(schema_path.display().to_string())
                .with_source_path(Some(schema_path)),
        })?;
    let schema_label = safe_schema_path.display().to_string();
    let schema_text =
        std::fs::read_to_string(&safe_schema_path).map_err(|e| FlowError::Internal {
            message: format!("schema read from {schema_label}: {e}"),
            location: FlowErrorLocation::at_path(schema_label.clone())
                .with_source_path(Some(&safe_schema_path)),
        })?;
    load_with_schema_text(
        yaml,
        &schema_text,
        schema_label,
        Some(&safe_schema_path),
        source_label,
        None,
    )
}

pub(crate) fn load_with_schema_text(
    yaml: &str,
    schema_text: &str,
    schema_label: impl Into<String>,
    schema_path: Option<&Path>,
    source_label: impl Into<String>,
    source_path: Option<&Path>,
) -> Result<FlowDoc> {
    let schema_label = schema_label.into();
    let source_label = source_label.into();
    let v_yaml: serde_yaml_bw::Value =
        serde_yaml_bw::from_str(yaml).map_err(|e| FlowError::Yaml {
            message: e.to_string(),
            location: yaml_error_location(&source_label, source_path, e.location()),
        })?;
    let v_json: Value = serde_json::to_value(&v_yaml).map_err(|e| FlowError::Internal {
        message: format!("yaml->json: {e}"),
        location: FlowErrorLocation::at_path(source_label.clone()).with_source_path(source_path),
    })?;
    validate_json(
        &v_json,
        schema_text,
        &schema_label,
        schema_path,
        &source_label,
        source_path,
    )?;

    let mut flow: FlowDoc = serde_yaml_bw::from_str(yaml).map_err(|e| FlowError::Yaml {
        message: e.to_string(),
        location: yaml_error_location(&source_label, source_path, e.location()),
    })?;

    let node_ids: Vec<String> = flow.nodes.keys().cloned().collect();
    for id in &node_ids {
        let node = flow.nodes.get_mut(id).ok_or_else(|| FlowError::Internal {
            message: format!("node '{id}' missing after load"),
            location: node_location(&source_label, source_path, id),
        })?;

        let mut component_kv: Option<(String, Value)> = None;
        let mut routing: Option<Value> = None;
        for (key, value) in &node.raw {
            if key == "routing" {
                routing = Some(value.clone());
                continue;
            }
            if component_kv.is_some() {
                return Err(FlowError::NodeComponentShape {
                    node_id: id.clone(),
                    location: node_location(&source_label, source_path, id),
                });
            }
            component_kv = Some((key.clone(), value.clone()));
        }

        let (component_key, payload) =
            component_kv.ok_or_else(|| FlowError::NodeComponentShape {
                node_id: id.clone(),
                location: node_location(&source_label, source_path, id),
            })?;
        if !is_valid_component_key(&component_key) {
            return Err(FlowError::BadComponentKey {
                component: component_key,
                node_id: id.clone(),
                location: node_location(&source_label, source_path, id),
            });
        }

        node.component = component_key;
        node.payload = payload;
        if let Some(value) = routing {
            node.routing = serde_json::from_value(value).map_err(|e| FlowError::Internal {
                message: format!("routing decode in node '{id}': {e}"),
                location: node_location(&source_label, source_path, id),
            })?;
        }
        node.raw = BTreeMap::new();
    }

    for (from_id, node) in &flow.nodes {
        for route in &node.routing {
            if let Some(to) = &route.to
                && to != "out"
                && !flow.nodes.contains_key(to)
            {
                return Err(FlowError::MissingNode {
                    target: to.clone(),
                    node_id: from_id.clone(),
                    location: routing_location(&source_label, source_path, from_id),
                });
            }
        }
    }

    if flow.start.is_none() && flow.nodes.contains_key("in") {
        flow.start = Some("in".to_string());
    }

    Ok(flow)
}

fn node_location(
    source_label: &str,
    source_path: Option<&Path>,
    node_id: &str,
) -> FlowErrorLocation {
    FlowErrorLocation::at_path(format!("{source_label}::nodes.{node_id}"))
        .with_source_path(source_path)
}

fn routing_location(
    source_label: &str,
    source_path: Option<&Path>,
    node_id: &str,
) -> FlowErrorLocation {
    FlowErrorLocation::at_path(format!("{source_label}::nodes.{node_id}.routing"))
        .with_source_path(source_path)
}

fn yaml_error_location(
    source_label: &str,
    source_path: Option<&Path>,
    loc: Option<YamlLocation>,
) -> FlowErrorLocation {
    if let Some(loc) = loc {
        FlowErrorLocation::at_path_with_position(
            source_label.to_string(),
            Some(loc.line()),
            Some(loc.column()),
        )
        .with_source_path(source_path)
    } else {
        FlowErrorLocation::at_path(source_label.to_string()).with_source_path(source_path)
    }
}
