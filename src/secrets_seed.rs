use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64_STANDARD;
use serde::Deserialize;
use serde_json::Value as JsonValue;
use serde_yaml_bw as serde_yaml;

#[derive(Debug, Deserialize)]
struct SeedDoc {
    entries: Vec<SeedEntry>,
}

#[derive(Debug, Deserialize)]
struct SeedEntry {
    uri: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    json: Option<JsonValue>,
    #[serde(default, rename = "bytes_b64")]
    bytes_b64: Option<String>,
    #[serde(default)]
    value: Option<JsonValue>,
}

pub fn load_seed_file(path: &Path) -> Result<HashMap<String, Vec<u8>>> {
    let data = fs::read_to_string(path)
        .with_context(|| format!("failed to read secrets seed at {}", path.display()))?;
    // Try canonical seed format first.
    if let Ok(doc) = serde_yaml::from_str::<SeedDoc>(&data) {
        let mut map = HashMap::new();
        for entry in doc.entries {
            let (uri, bytes) = seed_entry_to_bytes(entry)?;
            map.insert(uri, bytes);
        }
        return Ok(map);
    }
    // Fallback: simple map of uri -> string/JSON value.
    if let Ok(map) = serde_yaml::from_str::<HashMap<String, JsonValue>>(&data) {
        let mut out = HashMap::new();
        for (uri, val) in map {
            let bytes = match val {
                JsonValue::String(s) => s.into_bytes(),
                other => serde_json::to_vec(&other)
                    .context("failed to serialize seed value to JSON bytes")?,
            };
            out.insert(uri, bytes);
        }
        return Ok(out);
    }

    bail!("failed to parse secrets seed (unsupported format)")
}

fn seed_entry_to_bytes(entry: SeedEntry) -> Result<(String, Vec<u8>)> {
    let bytes = if let Some(text) = entry.text {
        text.into_bytes()
    } else if let Some(json) = entry.json {
        serde_json::to_vec(&json).context("failed to serialize seed json value")?
    } else if let Some(b64) = entry.bytes_b64 {
        B64_STANDARD
            .decode(b64.as_bytes())
            .context("failed to decode seed bytes_b64")?
    } else if let Some(val) = entry.value {
        match val {
            JsonValue::String(s) => s.into_bytes(),
            other => serde_json::to_vec(&other).context("failed to serialize seed value")?,
        }
    } else {
        bail!("seed entry {} missing value", entry.uri);
    };
    Ok((entry.uri, bytes))
}
