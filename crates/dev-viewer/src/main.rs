use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use dev_runner::FlowTranscript;
use serde_yaml_bw::{Mapping, Value as YamlValue};

#[derive(Parser)]
#[command(name = "dev-viewer")]
#[command(version)]
#[command(about = "Inspect Greentic flow transcripts with schema context")]
struct Cli {
    /// Path to the transcript file
    #[arg(short = 'f', long = "file")]
    file: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let transcript = load_transcript(&cli.file)?;

    println!(
        "Transcript for flow `{}` ({})",
        transcript.flow_name, transcript.flow_path
    );
    println!("Generated at: {}", transcript.generated_at);
    println!();

    for node in &transcript.nodes {
        render_node(node);
        println!();
    }

    Ok(())
}

fn load_transcript(path: &PathBuf) -> Result<FlowTranscript> {
    let content = fs::read_to_string(path)?;
    let transcript: FlowTranscript = serde_yaml_bw::from_str(&content)?;
    Ok(transcript)
}

fn render_node(node: &dev_runner::NodeTranscript) {
    println!("Node: {}", node.node_name);
    if let Some(schema_id) = &node.schema_id {
        println!("  Schema ID: {}", schema_id);
    } else {
        println!("  Schema ID: <unknown>");
    }

    let (overrides, defaults) = classify_paths(&node.run_log);
    println!("  Resolved configuration:");
    let mut path = Vec::new();
    print_value(&node.resolved_config, &mut path, &overrides, &defaults, 4);
}

fn classify_paths(run_log: &[String]) -> (HashSet<String>, HashSet<String>) {
    let mut overrides = HashSet::new();
    let mut defaults = HashSet::new();

    for entry in run_log {
        if let Some(rest) = entry.strip_prefix("override: ") {
            overrides.insert(rest.trim().to_string());
        } else if let Some(rest) = entry.strip_prefix("default: ") {
            defaults.insert(rest.trim().to_string());
        }
    }

    (overrides, defaults)
}

fn print_value(
    value: &YamlValue,
    path: &mut Vec<String>,
    overrides: &HashSet<String>,
    defaults: &HashSet<String>,
    indent: usize,
) {
    match value {
        YamlValue::Mapping(map) => print_mapping(map, path, overrides, defaults, indent),
        YamlValue::Sequence(seq) => print_sequence(seq, path, overrides, defaults, indent),
        _ => {
            let label = label_for(path, overrides, defaults);
            let rendered = render_scalar(value);
            println!(
                "{:indent$}{}{}",
                "",
                rendered,
                label.map(|l| format!(" ({l})")).unwrap_or_default(),
                indent = indent
            );
        }
    }
}

fn print_mapping(
    map: &Mapping,
    path: &mut Vec<String>,
    overrides: &HashSet<String>,
    defaults: &HashSet<String>,
    indent: usize,
) {
    for (key, value) in map {
        let key_segment = key_to_segment(key);
        path.push(key_segment.clone());
        let label_suffix = label_for(path, overrides, defaults)
            .map(|kind| format!(" ({kind})"))
            .unwrap_or_default();
        match value {
            YamlValue::Mapping(_) => {
                println!(
                    "{:indent$}{}:{label_suffix}",
                    "",
                    key_segment,
                    indent = indent
                );
                print_value(value, path, overrides, defaults, indent + 2);
            }
            YamlValue::Sequence(seq) => {
                println!(
                    "{:indent$}{}:{label_suffix}",
                    "",
                    key_segment,
                    indent = indent
                );
                print_sequence(seq, path, overrides, defaults, indent + 2);
            }
            _ => {
                let rendered = render_scalar(value);
                println!(
                    "{:indent$}{}: {}{label_suffix}",
                    "",
                    key_segment,
                    rendered,
                    indent = indent
                );
            }
        }
        path.pop();
    }
}

fn print_sequence(
    seq: &[YamlValue],
    path: &mut Vec<String>,
    overrides: &HashSet<String>,
    defaults: &HashSet<String>,
    indent: usize,
) {
    for (index, value) in seq.iter().enumerate() {
        let label_suffix = label_for(path, overrides, defaults)
            .map(|kind| format!(" ({kind})"))
            .unwrap_or_default();
        let indent_str = " ".repeat(indent);
        match value {
            YamlValue::Mapping(_) | YamlValue::Sequence(_) => {
                println!("{indent_str}- {label_suffix}");
                path.push(index.to_string());
                print_value(value, path, overrides, defaults, indent + 2);
                path.pop();
            }
            _ => {
                let rendered = render_scalar(value);
                if label_suffix.is_empty() {
                    println!("{indent_str}- {rendered}");
                } else {
                    println!("{indent_str}- {rendered}{label_suffix}");
                }
            }
        }
    }
}

fn label_for<'a>(
    path: &[String],
    overrides: &'a HashSet<String>,
    defaults: &'a HashSet<String>,
) -> Option<&'a str> {
    if path.is_empty() {
        return None;
    }

    let joined = path.join(".");
    if overrides.contains(&joined) {
        Some("override")
    } else if defaults.contains(&joined) {
        Some("default")
    } else {
        None
    }
}

fn key_to_segment(key: &YamlValue) -> String {
    if let Some(as_str) = key.as_str() {
        as_str.to_string()
    } else {
        serde_yaml_bw::to_string(key)
            .unwrap_or_else(|_| "<non-string>".to_string())
            .trim_matches('\n')
            .to_string()
    }
}

fn render_scalar(value: &YamlValue) -> String {
    match value {
        YamlValue::Null(_) => "null".to_string(),
        YamlValue::Bool(b, _) => b.to_string(),
        YamlValue::Number(number, _) => number.to_string(),
        YamlValue::String(s, _) => s.clone(),
        _ => serde_yaml_bw::to_string(value)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}
