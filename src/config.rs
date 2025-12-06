use std::collections::HashMap;
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
    #[allow(dead_code)]
    #[serde(default)]
    pub distributor: DistributorSection,
}

#[derive(Debug, Default, Deserialize)]
pub struct ToolsSection {
    #[serde(rename = "greentic-component", default)]
    pub greentic_component: ToolEntry,
    #[serde(rename = "packc", default)]
    pub packc: ToolEntry,
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

#[derive(Debug, Default, Deserialize)]
pub struct DistributorSection {
    /// Map of profile name -> profile configuration.
    #[allow(dead_code)]
    #[serde(default, flatten)]
    pub profiles: HashMap<String, DistributorProfileConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DistributorProfileConfig {
    #[allow(dead_code)]
    pub url: String,
    #[allow(dead_code)]
    #[serde(default)]
    pub token: Option<String>,
}

pub fn load() -> Result<GreenticConfig> {
    let path_override = std::env::var("GREENTIC_CONFIG").ok();
    load_from(path_override.as_deref())
}

pub fn load_from(path_override: Option<&str>) -> Result<GreenticConfig> {
    let Some(path) = config_path_override(path_override) else {
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

fn config_path_override(path_override: Option<&str>) -> Option<PathBuf> {
    if let Some(raw) = path_override {
        return Some(PathBuf::from(raw));
    }
    config_path()
}

pub fn config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|mut home| {
        home.push(".greentic");
        home.push("config.toml");
        home
    })
}
