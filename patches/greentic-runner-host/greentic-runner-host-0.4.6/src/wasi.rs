use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtx, WasiCtxBuilder};

/// Specification for exposing a host directory to the guest.
#[derive(Clone, Debug)]
pub struct PreopenSpec {
    pub host_path: PathBuf,
    pub guest_path: String,
    pub read_only: bool,
}

impl PreopenSpec {
    pub fn new(host_path: impl Into<PathBuf>, guest_path: impl Into<String>) -> Self {
        Self {
            host_path: host_path.into(),
            guest_path: guest_path.into(),
            read_only: false,
        }
    }

    pub fn read_only(mut self, value: bool) -> Self {
        self.read_only = value;
        self
    }

    fn validate(&self) -> Result<()> {
        let meta = fs::metadata(&self.host_path)
            .with_context(|| format!("failed to stat preopen {}", self.host_path.display()))?;
        if !meta.is_dir() {
            return Err(anyhow!(
                "preopen {} must point to a directory",
                self.host_path.display()
            ));
        }
        Ok(())
    }
}

/// Policy describing which WASI capabilities are surfaced into packs.
#[derive(Clone, Debug)]
pub struct RunnerWasiPolicy {
    pub inherit_stdio: bool,
    pub env_allow: Vec<String>,
    pub env_set: HashMap<String, String>,
    pub preopens: Vec<PreopenSpec>,
}

impl Default for RunnerWasiPolicy {
    fn default() -> Self {
        Self {
            inherit_stdio: true,
            env_allow: Vec::new(),
            env_set: HashMap::new(),
            preopens: Vec::new(),
        }
    }
}

impl RunnerWasiPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allow_env(mut self, key: impl Into<String>) -> Self {
        self.env_allow.push(key.into());
        self
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env_set.insert(key.into(), value.into());
        self
    }

    pub fn with_preopen(mut self, spec: PreopenSpec) -> Self {
        self.preopens.push(spec);
        self
    }

    pub fn inherit_stdio(mut self, inherit: bool) -> Self {
        self.inherit_stdio = inherit;
        self
    }

    pub(crate) fn instantiate(&self) -> Result<WasiCtx> {
        let mut builder = WasiCtxBuilder::new();
        if self.inherit_stdio {
            builder.inherit_stdio();
        }
        let env_pairs = self.collect_env();
        if !env_pairs.is_empty() {
            let borrowed = env_pairs
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect::<Vec<_>>();
            builder.envs(&borrowed);
        }
        for spec in &self.preopens {
            self.install_preopen(&mut builder, spec)?;
        }
        Ok(builder.build())
    }

    fn collect_env(&self) -> Vec<(String, String)> {
        let mut envs = Vec::new();
        for key in &self.env_allow {
            if let Ok(value) = std::env::var(key) {
                envs.push((key.clone(), value));
            }
        }
        for (key, value) in &self.env_set {
            // Remove any previously allowed env so explicit overrides win.
            if let Some(pos) = envs.iter().position(|(existing, _)| existing == key) {
                envs.remove(pos);
            }
            envs.push((key.clone(), value.clone()));
        }
        envs
    }

    fn install_preopen(&self, builder: &mut WasiCtxBuilder, spec: &PreopenSpec) -> Result<()> {
        spec.validate()?;
        let (dir_perms, file_perms) = if spec.read_only {
            (DirPerms::READ, FilePerms::READ)
        } else {
            (DirPerms::all(), FilePerms::all())
        };
        builder
            .preopened_dir(&spec.host_path, &spec.guest_path, dir_perms, file_perms)
            .with_context(|| {
                format!(
                    "failed to preopen {} as {}",
                    spec.host_path.display(),
                    spec.guest_path
                )
            })?;
        Ok(())
    }
}
