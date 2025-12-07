use crate::{
    ir::{FlowIR, NodeKind, classify_node_type},
    registry::AdapterCatalog,
};

#[derive(Clone, Debug, Default)]
pub struct AdapterResolvableRule;

impl AdapterResolvableRule {
    pub fn check(flow: &FlowIR, catalog: &AdapterCatalog) -> Vec<String> {
        let mut errors = Vec::new();
        for (idx, (node_id, node)) in flow.nodes.iter().enumerate() {
            match classify_node_type(&node.component) {
                NodeKind::Adapter {
                    namespace,
                    adapter,
                    operation,
                } => {
                    if !catalog.contains(&namespace, &adapter, &operation) {
                        errors.push(format!(
                            "adapter_resolvable: node #{idx} ('{node_id}') component '{}' missing adapter '{}.{}' operation '{}'",
                            node.component, namespace, adapter, operation
                        ));
                    }
                }
                NodeKind::Builtin(_) => {}
            }
        }
        errors
    }
}
