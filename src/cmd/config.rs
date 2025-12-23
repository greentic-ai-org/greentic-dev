use std::fs;
use std::path::Path;

use crate::cli::{ConfigCommand, ConfigSetArgs};
use anyhow::{Context, Result, anyhow, bail};
use toml_edit::{DocumentMut, Item, Table, value};

use crate::config;

pub fn run(command: ConfigCommand) -> Result<()> {
    match command {
        ConfigCommand::Set(args) => set_value(&args),
    }
}

fn set_value(args: &ConfigSetArgs) -> Result<()> {
    let path = match &args.file {
        Some(path) => path.clone(),
        None => config::config_path().ok_or_else(|| {
            anyhow!("failed to resolve default config path (no home directory found)")
        })?,
    };

    ensure_parent(&path)?;

    let mut doc = if path.exists() {
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if raw.trim().is_empty() {
            DocumentMut::new()
        } else {
            raw.parse::<DocumentMut>()
                .with_context(|| format!("failed to parse {}", path.display()))?
        }
    } else {
        DocumentMut::new()
    };

    apply_key(&mut doc, &args.key, &args.value)?;

    fs::write(&path, doc.to_string())
        .with_context(|| format!("failed to write {}", path.display()))?;
    println!("Updated {}", path.display());
    Ok(())
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    Ok(())
}

fn apply_key(doc: &mut DocumentMut, key: &str, value_str: &str) -> Result<()> {
    let segments = key
        .split('.')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.is_empty() {
        bail!("config key cannot be empty");
    }

    let mut current = doc.as_table_mut();
    for segment in &segments[..segments.len() - 1] {
        current = current
            .entry(segment)
            .or_insert(Item::Table(Table::new()))
            .as_table_mut()
            .ok_or_else(|| anyhow!("path `{segment}` is not a table in the config"))?;
    }

    current.insert(segments.last().unwrap(), value(value_str));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn creates_new_document() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("config.toml");
        let args = ConfigSetArgs {
            key: "defaults.component.org".into(),
            value: "ai.greentic".into(),
            file: Some(path.clone()),
        };
        set_value(&args).unwrap();
        let written = fs::read_to_string(path).unwrap();
        assert!(written.contains("defaults"));
        assert!(written.contains("ai.greentic"));
    }

    #[test]
    fn updates_nested_tables() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("config.toml");
        fs::write(
            &path,
            r#"
[defaults]
[defaults.component]
org = "ai.greentic"
"#,
        )
        .unwrap();

        let args = ConfigSetArgs {
            key: "defaults.component.template".into(),
            value: "rust-wasi".into(),
            file: Some(path.clone()),
        };
        set_value(&args).unwrap();
        let written = fs::read_to_string(path).unwrap();
        assert!(written.contains("template = \"rust-wasi\""));
    }
}
