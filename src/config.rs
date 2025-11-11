use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub struct GreenticConfig {
    #[serde(default)]
    pub tools: ToolsSection,
    #[allow(dead_code)]
    #[serde(default)]
    pub defaults: DefaultsSection,
}

#[derive(Debug, Default, Deserialize)]
pub struct ToolsSection {
    #[serde(rename = "greentic-component", default)]
    pub greentic_component: ToolEntry,
}

#[derive(Debug, Default, Deserialize)]
pub struct ToolEntry {
    pub path: Option<PathBuf>,
}

#[allow(dead_code)]
#[derive(Debug, Default, Deserialize)]
pub struct DefaultsSection {
    #[serde(default)]
    pub component: ComponentDefaults,
}

#[allow(dead_code)]
#[derive(Debug, Default, Deserialize)]
pub struct ComponentDefaults {
    pub org: Option<String>,
    pub template: Option<String>,
}

pub fn load() -> Result<GreenticConfig> {
    let Some(path) = config_path() else {
        return Ok(GreenticConfig::default());
    };

    if !path.exists() {
        return Ok(GreenticConfig::default());
    }

    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read config at {}", path.display()))?;
    let config: GreenticConfig = toml::from_str(&raw)
        .with_context(|| format!("failed to parse config at {}", path.display()))?;
    Ok(config)
}

pub fn config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|mut home| {
        home.push(".greentic");
        home.push("config.toml");
        home
    })
}
