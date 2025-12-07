//! Downstream runtimes must set the current tenant telemetry context via
//! `greentic_types::telemetry::set_current_tenant_ctx` before executing flows
//! (for example, prior to `FlowEngine::run` in the host runner).
#![forbid(unsafe_code)]
#![allow(clippy::result_large_err)]

pub mod config_flow;
pub mod error;
pub mod flow_bundle;
pub mod ir;
pub mod json_output;
pub mod lint;
pub mod loader;
pub mod model;
pub mod path_safety;
pub mod registry;
pub mod resolve;
pub mod util;

pub use flow_bundle::{
    ComponentPin, FlowBundle, NodeRef, blake3_hex, canonicalize_json, extract_component_pins,
    load_and_validate_bundle, load_and_validate_bundle_with_ir,
};
pub use json_output::{JsonDiagnostic, LintJsonOutput, lint_to_stdout_json};

use crate::{
    error::Result,
    ir::{FlowIR, NodeIR, RouteIR},
    model::FlowDoc,
};
use indexmap::IndexMap;

/// Convert a `FlowDoc` into its compact intermediate representation.
pub fn to_ir(flow: FlowDoc) -> Result<FlowIR> {
    let mut nodes: IndexMap<String, NodeIR> = IndexMap::new();
    for (id, node) in flow.nodes {
        nodes.insert(
            id,
            NodeIR {
                component: node.component,
                payload_expr: node.payload,
                routes: node
                    .routing
                    .into_iter()
                    .map(|route| RouteIR {
                        to: route.to,
                        out: route.out.unwrap_or(false),
                    })
                    .collect(),
            },
        );
    }

    Ok(FlowIR {
        id: flow.id,
        flow_type: flow.flow_type,
        start: flow.start,
        parameters: flow.parameters,
        nodes,
    })
}
