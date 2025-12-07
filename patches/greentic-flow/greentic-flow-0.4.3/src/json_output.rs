use crate::{
    error::{FlowError, FlowErrorLocation},
    flow_bundle::{FlowBundle, load_and_validate_bundle_with_ir},
    lint::lint_builtin_rules,
};
use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
pub struct JsonDiagnostic {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub col: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_pointer: Option<String>,
}

impl JsonDiagnostic {
    pub fn from_location(message: String, location: FlowErrorLocation) -> Self {
        let FlowErrorLocation {
            path,
            source_path,
            line,
            col,
            json_pointer,
        } = location;
        JsonDiagnostic {
            message,
            source_path: source_path
                .as_ref()
                .map(|p| p.display().to_string())
                .or(path),
            line,
            col,
            json_pointer,
        }
    }

    pub fn from_message(message: String, source_path: Option<String>) -> Self {
        JsonDiagnostic {
            message,
            source_path,
            line: None,
            col: None,
            json_pointer: None,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct LintJsonOutput {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle: Option<FlowBundle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash_blake3: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<JsonDiagnostic>,
}

impl LintJsonOutput {
    pub fn success(bundle: FlowBundle) -> Self {
        let hash = bundle.hash_blake3.clone();
        LintJsonOutput {
            ok: true,
            hash_blake3: Some(hash),
            bundle: Some(bundle),
            errors: Vec::new(),
        }
    }

    pub fn lint_failure(messages: Vec<String>, source_path: Option<String>) -> Self {
        let errors = messages
            .into_iter()
            .map(|message| JsonDiagnostic::from_message(message, source_path.clone()))
            .collect();
        LintJsonOutput {
            ok: false,
            bundle: None,
            hash_blake3: None,
            errors,
        }
    }

    pub fn error(err: FlowError) -> Self {
        LintJsonOutput {
            ok: false,
            bundle: None,
            hash_blake3: None,
            errors: flow_error_to_reports(err),
        }
    }

    pub fn into_string(self) -> String {
        serde_json::to_string(&self).expect("lint output serialization")
    }
}

pub fn flow_error_to_reports(err: FlowError) -> Vec<JsonDiagnostic> {
    let display_message = err.to_string();
    match err {
        FlowError::Schema {
            details, location, ..
        } => {
            if details.is_empty() {
                vec![JsonDiagnostic::from_location(display_message, location)]
            } else {
                details
                    .into_iter()
                    .map(|detail| JsonDiagnostic::from_location(detail.message, detail.location))
                    .collect()
            }
        }
        FlowError::Yaml { location, .. }
        | FlowError::NodeComponentShape { location, .. }
        | FlowError::BadComponentKey { location, .. }
        | FlowError::MissingNode { location, .. }
        | FlowError::Internal { location, .. } => {
            vec![JsonDiagnostic::from_location(display_message, location)]
        }
    }
}

/// Produce the same JSON emitted by `ygtc-lint --json` for builtin linting.
pub fn lint_to_stdout_json(ygtc: &str) -> String {
    match load_and_validate_bundle_with_ir(ygtc, None) {
        Ok((bundle, ir)) => {
            let lint_errors = lint_builtin_rules(&ir);
            if lint_errors.is_empty() {
                LintJsonOutput::success(bundle).into_string()
            } else {
                LintJsonOutput::lint_failure(lint_errors, None).into_string()
            }
        }
        Err(err) => LintJsonOutput::error(err).into_string(),
    }
}
