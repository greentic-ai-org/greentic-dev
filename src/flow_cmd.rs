use std::fs;
use std::path::Path;

use crate::path_safety::normalize_under_root;
use anyhow::{Context, Result};
use greentic_flow::flow_bundle::load_and_validate_bundle;

pub fn validate(path: &Path, compact_json: bool) -> Result<()> {
    let root = std::env::current_dir()
        .context("failed to resolve workspace root")?
        .canonicalize()
        .context("failed to canonicalize workspace root")?;
    let safe = normalize_under_root(&root, path)?;
    let source = fs::read_to_string(&safe)
        .with_context(|| format!("failed to read flow definition at {}", safe.display()))?;

    let bundle = load_and_validate_bundle(&source, Some(&safe)).with_context(|| {
        format!(
            "flow validation failed for {} using greentic-flow",
            safe.display()
        )
    })?;

    let serialized = if compact_json {
        serde_json::to_string(&bundle)?
    } else {
        serde_json::to_string_pretty(&bundle)?
    };

    println!("{serialized}");
    Ok(())
}
