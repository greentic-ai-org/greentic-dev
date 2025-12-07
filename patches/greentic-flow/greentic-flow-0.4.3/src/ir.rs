use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Classification of a node's component type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    /// A node backed by an adapter operation in the form `<namespace>.<adapter>.<operation>`.
    Adapter {
        namespace: String,
        adapter: String,
        operation: String,
    },
    /// Any other node type that does not match the adapter convention.
    Builtin(String),
}

/// Classify a component string into [`NodeKind`].
pub fn classify_node_type(node_type: &str) -> NodeKind {
    let parts = node_type.split('.').collect::<Vec<_>>();
    if parts.len() >= 3 {
        let namespace = parts[0].to_string();
        let adapter = parts[1].to_string();
        let operation = parts[2..].join(".");
        NodeKind::Adapter {
            namespace,
            adapter,
            operation,
        }
    } else {
        NodeKind::Builtin(node_type.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowIR {
    pub id: String,
    pub flow_type: String,
    pub start: Option<String>,
    pub parameters: Value,
    pub nodes: IndexMap<String, NodeIR>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeIR {
    pub component: String,
    pub payload_expr: Value,
    pub routes: Vec<RouteIR>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteIR {
    pub to: Option<String>,
    pub out: bool,
}
