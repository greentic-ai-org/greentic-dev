use std::{fmt, path::PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowErrorLocation {
    pub path: Option<String>,
    pub source_path: Option<PathBuf>,
    pub line: Option<usize>,
    pub col: Option<usize>,
    pub json_pointer: Option<String>,
}

impl FlowErrorLocation {
    pub fn new<P: Into<Option<String>>>(path: P, line: Option<usize>, col: Option<usize>) -> Self {
        FlowErrorLocation {
            path: path.into(),
            line,
            col,
            source_path: None,
            json_pointer: None,
        }
    }

    pub fn at_path(path: impl Into<String>) -> Self {
        FlowErrorLocation::new(Some(path.into()), None, None)
    }

    pub fn at_path_with_position(
        path: impl Into<String>,
        line: Option<usize>,
        col: Option<usize>,
    ) -> Self {
        FlowErrorLocation::new(Some(path.into()), line, col)
    }

    pub fn with_source_path(mut self, source_path: Option<&std::path::Path>) -> Self {
        self.source_path = source_path.map(|p| p.to_path_buf());
        self
    }

    pub fn with_json_pointer(mut self, pointer: Option<impl Into<String>>) -> Self {
        self.json_pointer = pointer.map(|p| p.into());
        self
    }

    pub fn describe(&self) -> Option<String> {
        if self.path.is_none() && self.line.is_none() && self.col.is_none() {
            return None;
        }
        let mut parts = String::new();
        if let Some(path) = &self.path {
            parts.push_str(path);
        }
        match (self.line, self.col) {
            (Some(line), Some(column)) => {
                if !parts.is_empty() {
                    parts.push(':');
                }
                parts.push_str(&format!("{line}:{column}"));
            }
            (Some(line), None) => {
                if !parts.is_empty() {
                    parts.push(':');
                }
                parts.push_str(&line.to_string());
            }
            (None, Some(column)) => {
                if !parts.is_empty() {
                    parts.push(':');
                }
                parts.push_str(&column.to_string());
            }
            _ => {}
        }
        Some(parts)
    }
}

impl fmt::Display for FlowErrorLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_none() && self.line.is_none() && self.col.is_none() {
            return Ok(());
        }
        write!(f, " at ")?;
        if let Some(path) = &self.path {
            write!(f, "{path}")?;
            if self.line.is_some() || self.col.is_some() {
                write!(f, ":")?;
            }
        }
        match (self.line, self.col) {
            (Some(line), Some(column)) => write!(f, "{line}:{column}")?,
            (Some(line), None) => write!(f, "{line}")?,
            (None, Some(column)) => write!(f, "{column}")?,
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaErrorDetail {
    pub message: String,
    pub location: FlowErrorLocation,
}

#[derive(Debug, Error)]
pub enum FlowError {
    #[error("YAML parse error{location}: {message}")]
    Yaml {
        message: String,
        location: FlowErrorLocation,
    },
    #[error("Schema validation failed{location}:\n{message}")]
    Schema {
        message: String,
        details: Vec<SchemaErrorDetail>,
        location: FlowErrorLocation,
    },
    #[error(
        "Node '{node_id}' must contain exactly one component key like 'qa.process' plus optional 'routing'{location}"
    )]
    NodeComponentShape {
        node_id: String,
        location: FlowErrorLocation,
    },
    #[error(
        "Invalid component key '{component}' in node '{node_id}' (expected namespace.adapter.operation or builtin like 'questions'/'template'){location}"
    )]
    BadComponentKey {
        component: String,
        node_id: String,
        location: FlowErrorLocation,
    },
    #[error("Missing node '{target}' referenced in routing from '{node_id}'{location}")]
    MissingNode {
        target: String,
        node_id: String,
        location: FlowErrorLocation,
    },
    #[error("Internal error{location}: {message}")]
    Internal {
        message: String,
        location: FlowErrorLocation,
    },
}

#[allow(clippy::result_large_err)]
pub type Result<T> = std::result::Result<T, FlowError>;
