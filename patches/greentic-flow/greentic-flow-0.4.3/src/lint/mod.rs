mod adapter_resolvable;
use crate::ir::FlowIR;

pub use adapter_resolvable::AdapterResolvableRule;

use crate::registry::AdapterCatalog;

/// Run the built-in lint rules that do not require external data.
pub fn lint_builtin_rules(_flow: &FlowIR) -> Vec<String> {
    let mut errors = Vec::new();
    if let Some(start) = &_flow.start
        && !_flow.nodes.contains_key(start)
    {
        errors.push(format!(
            "start_node_exists: start node '{}' not found in nodes",
            start
        ));
    }
    errors
}

/// Run all lint rules including adapter resolution backed by a catalog.
pub fn lint_with_registry(flow: &FlowIR, catalog: &AdapterCatalog) -> Vec<String> {
    let mut errors = lint_builtin_rules(flow);
    errors.extend(AdapterResolvableRule::check(flow, catalog));
    errors
}
