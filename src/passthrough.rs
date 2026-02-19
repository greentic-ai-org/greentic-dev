use anyhow::{Context, Result, anyhow, bail};
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

/// Resolve a binary by name using env override, optional workspace target, then PATH.
pub fn resolve_binary(name: &str) -> Result<PathBuf> {
    let env_key = format!("GREENTIC_DEV_BIN_{}", name.replace('-', "_").to_uppercase());
    if let Ok(path) = env::var(&env_key) {
        let pb = PathBuf::from(path);
        if pb.exists() {
            return Ok(pb);
        }
        bail!("{env_key} points to non-existent binary: {}", pb.display());
    }

    // Optional workspace target resolution (debug and release) before PATH.
    // This keeps local dev/test runs pinned to the binaries built in this workspace.
    if let Ok(cwd) = env::current_dir() {
        for dir in ["target/debug", "target/release"] {
            let candidate = cwd.join(dir).join(name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    if let Ok(path) = which::which(name) {
        return Ok(path);
    }

    which::which(name).with_context(|| {
        format!("failed to find `{name}` in PATH; set {env_key} or install {name}")
    })
}

pub fn run_passthrough(bin: &Path, args: &[OsString], verbose: bool) -> Result<ExitStatus> {
    if verbose {
        eprintln!("greentic-dev passthrough -> {} {:?}", bin.display(), args);
        let _ = Command::new(bin)
            .arg("--version")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status();
    }

    Command::new(bin)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| anyhow!("failed to execute {}: {e}", bin.display()))
}
