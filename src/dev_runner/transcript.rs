use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_yaml_bw::{Mapping, Value as YamlValue};

use super::runner::ValidatedNode;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeTranscript {
    pub node_name: String,
    pub resolved_config: YamlValue,
    pub schema_id: Option<String>,
    pub run_log: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowTranscript {
    pub flow_name: String,
    pub flow_path: String,
    pub generated_at: u64,
    pub nodes: Vec<NodeTranscript>,
}

#[derive(Clone, Debug)]
pub struct TranscriptStore {
    root: PathBuf,
}

#[derive(Debug)]
pub enum TranscriptError {
    Io(std::io::Error),
    Serialize(serde_yaml_bw::Error),
}

impl TranscriptStore {
    pub fn with_root<P: Into<PathBuf>>(root: P) -> Self {
        Self { root: root.into() }
    }

    pub fn write_transcript<P>(
        &self,
        flow_path: P,
        transcript: &FlowTranscript,
    ) -> Result<PathBuf, TranscriptError>
    where
        P: AsRef<Path>,
    {
        let flow_path = flow_path.as_ref();
        let flow_stem = flow_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("flow");

        let output_path = self
            .root
            .join(format!("{}-{}.yaml", flow_stem, transcript.generated_at));

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let serialized = serde_yaml_bw::to_string(transcript)?;
        fs::write(&output_path, serialized)?;

        Ok(output_path)
    }
}

impl Default for TranscriptStore {
    fn default() -> Self {
        Self::with_root(".greentic/transcripts")
    }
}

impl FlowTranscript {
    pub fn from_validated_nodes<P: AsRef<Path>>(flow_path: P, nodes: &[ValidatedNode]) -> Self {
        let flow_path_ref = flow_path.as_ref();
        let flow_name = flow_path_ref
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("flow")
            .to_string();

        let generated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let node_transcripts = nodes.iter().map(node_transcript_from_validated).collect();

        Self {
            flow_name,
            flow_path: flow_path_ref.to_string_lossy().to_string(),
            generated_at,
            nodes: node_transcripts,
        }
    }
}

impl NodeTranscript {
    pub fn merged_config(&self) -> &YamlValue {
        &self.resolved_config
    }
}

impl fmt::Display for TranscriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TranscriptError::Io(error) => write!(f, "failed to write transcript: {error}"),
            TranscriptError::Serialize(error) => {
                write!(f, "failed to serialize transcript: {error}")
            }
        }
    }
}

impl Error for TranscriptError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            TranscriptError::Io(error) => Some(error),
            TranscriptError::Serialize(error) => Some(error),
        }
    }
}

impl From<std::io::Error> for TranscriptError {
    fn from(value: std::io::Error) -> Self {
        TranscriptError::Io(value)
    }
}

impl From<serde_yaml_bw::Error> for TranscriptError {
    fn from(value: serde_yaml_bw::Error) -> Self {
        TranscriptError::Serialize(value)
    }
}

fn node_transcript_from_validated(node: &ValidatedNode) -> NodeTranscript {
    let (resolved_config, run_log) = merge_with_defaults(node.defaults.as_ref(), &node.node_config);
    let node_name = node_name(&node.node_config, &node.component);

    NodeTranscript {
        node_name,
        resolved_config,
        schema_id: node.schema_id.clone(),
        run_log,
    }
}

fn node_name(node_config: &YamlValue, fallback: &str) -> String {
    node_config
        .as_mapping()
        .and_then(|mapping| mapping.get("id"))
        .and_then(|value| value.as_str())
        .unwrap_or(fallback)
        .to_string()
}

fn merge_with_defaults(
    defaults: Option<&YamlValue>,
    overrides: &YamlValue,
) -> (YamlValue, Vec<String>) {
    let mut run_log = Vec::new();
    let mut path = Vec::new();
    let resolved = merge_node(defaults, overrides, &mut path, &mut run_log);

    // Deduplicate logs while preserving insertion order.
    let mut seen = HashSet::new();
    run_log.retain(|entry| seen.insert(entry.clone()));

    (resolved, run_log)
}

fn merge_node(
    defaults: Option<&YamlValue>,
    overrides: &YamlValue,
    path: &mut Vec<String>,
    run_log: &mut Vec<String>,
) -> YamlValue {
    match (defaults, overrides) {
        (Some(YamlValue::Mapping(default_map)), YamlValue::Mapping(override_map)) => {
            let mut result = Mapping::new();

            for (key, default_value) in default_map {
                let key_str = key_to_segment(key);
                path.push(key_str.clone());
                if let Some(override_value) = override_map.get(key) {
                    let merged = merge_node(Some(default_value), override_value, path, run_log);
                    result.insert(key.clone(), merged);
                } else {
                    log_default(path, run_log);
                    result.insert(key.clone(), default_value.clone());
                }
                path.pop();
            }

            for (key, override_value) in override_map {
                if default_map.contains_key(key) {
                    continue;
                }
                let key_str = key_to_segment(key);
                path.push(key_str.clone());
                log_override(path, run_log);
                let merged = merge_node(None, override_value, path, run_log);
                result.insert(key.clone(), merged);
                path.pop();
            }

            YamlValue::Mapping(result)
        }
        (Some(YamlValue::Sequence(default_seq)), YamlValue::Sequence(override_seq)) => {
            if let Some(path_str) = path_string(path) {
                if default_seq == override_seq {
                    run_log.push(format!("default: {path_str}"));
                } else {
                    run_log.push(format!("override: {path_str}"));
                }
            }
            YamlValue::Sequence(override_seq.clone())
        }
        (Some(default_value), override_value) => {
            if let Some(path_str) = path_string(path) {
                if default_value == override_value {
                    run_log.push(format!("default: {path_str}"));
                } else {
                    run_log.push(format!("override: {path_str}"));
                }
            }
            override_value.clone()
        }
        (None, override_value) => {
            if let Some(path_str) = path_string(path) {
                run_log.push(format!("override: {path_str}"));
            }
            override_value.clone()
        }
    }
}

fn log_default(path: &[String], run_log: &mut Vec<String>) {
    if let Some(path_str) = path_string(path) {
        run_log.push(format!("default: {path_str}"));
    }
}

fn log_override(path: &[String], run_log: &mut Vec<String>) {
    if let Some(path_str) = path_string(path) {
        run_log.push(format!("override: {path_str}"));
    }
}

fn path_string(path: &[String]) -> Option<String> {
    if path.is_empty() {
        None
    } else {
        Some(path.join("."))
    }
}

fn key_to_segment(key: &YamlValue) -> String {
    key.as_str()
        .map(|s| s.to_string())
        .or_else(|| key.as_u64().map(|n| n.to_string()))
        .unwrap_or_else(|| "unknown".to_string())
}
