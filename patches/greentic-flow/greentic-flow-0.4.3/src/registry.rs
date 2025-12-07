use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::Path,
};

use crate::path_safety::normalize_under_root;

/// Catalog of known adapters and their supported operations.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct AdapterCatalog {
    /// Map of `<namespace>.<adapter>` to the operations that adapter exposes.
    pub adapters: HashMap<String, HashSet<String>>,
}

impl AdapterCatalog {
    /// Load a registry from disk, accepting JSON by default and TOML when the `toml` feature is enabled.
    pub fn load_from_file(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path_ref = path.as_ref();
        let registry_root = env::current_dir().context("unable to resolve registry root")?;
        let safe_path = normalize_under_root(&registry_root, path_ref)?;
        let txt = fs::read_to_string(&safe_path).with_context(|| {
            format!("unable to read adapter registry at {}", safe_path.display())
        })?;
        if let Ok(value) = serde_json::from_str::<Self>(&txt) {
            return Ok(value);
        }

        #[cfg(feature = "toml")]
        {
            if let Ok(value) = toml::from_str::<Self>(&txt) {
                return Ok(value);
            }
        }

        #[cfg(feature = "toml")]
        {
            anyhow::bail!(
                "unsupported registry format in {}: expected JSON or TOML",
                path_ref.display()
            );
        }

        #[cfg(not(feature = "toml"))]
        {
            anyhow::bail!(
                "unsupported registry format in {}: expected JSON (enable `toml` feature for TOML support)",
                path_ref.display()
            );
        }
    }

    /// Check if the catalog contains the given adapter operation.
    pub fn contains(&self, namespace: &str, adapter: &str, operation: &str) -> bool {
        let key = format!("{namespace}.{adapter}");
        self.adapters
            .get(&key)
            .map(|ops| ops.contains(operation))
            .unwrap_or(false)
    }
}
