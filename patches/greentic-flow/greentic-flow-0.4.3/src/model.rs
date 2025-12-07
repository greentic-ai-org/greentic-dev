use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

fn default_parameters() -> Value {
    Value::Object(Default::default())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowDoc {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub flow_type: String,
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default = "default_parameters")]
    pub parameters: Value,
    pub nodes: BTreeMap<String, Node>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Node {
    #[serde(default)]
    pub component: String,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub routing: Vec<Route>,
    #[serde(flatten, default)]
    pub raw: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Route {
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub out: Option<bool>,
}
