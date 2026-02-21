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

    if auto_install_enabled()
        && let Some(spec) = install_spec(name)
    {
        install_with_binstall(spec)?;
        if let Ok(path) = which::which(name) {
            return Ok(path);
        }
    }

    bail!("failed to find `{name}` in PATH; set {env_key} or install {name}")
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

#[derive(Clone, Copy)]
struct InstallSpec {
    crate_name: &'static str,
    bin_name: &'static str,
}

fn install_spec(name: &str) -> Option<InstallSpec> {
    let spec = match name {
        "greentic-component" => InstallSpec {
            crate_name: "greentic-component",
            bin_name: "greentic-component",
        },
        "greentic-flow" => InstallSpec {
            crate_name: "greentic-flow",
            bin_name: "greentic-flow",
        },
        "greentic-pack" => InstallSpec {
            crate_name: "greentic-pack",
            bin_name: "greentic-pack",
        },
        "greentic-runner-cli" => InstallSpec {
            crate_name: "greentic-runner",
            bin_name: "greentic-runner-cli",
        },
        "greentic-gui" => InstallSpec {
            crate_name: "greentic-gui",
            bin_name: "greentic-gui",
        },
        "greentic-secrets" => InstallSpec {
            crate_name: "greentic-secrets",
            bin_name: "greentic-secrets",
        },
        _ => return None,
    };
    Some(spec)
}

fn auto_install_enabled() -> bool {
    auto_install_enabled_from_env(env::var("GREENTIC_DEV_AUTO_INSTALL").ok().as_deref())
}

fn auto_install_enabled_from_env(value: Option<&str>) -> bool {
    value
        .map(|v| {
            !matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "no" | "off"
            )
        })
        .unwrap_or(true)
}

fn install_with_binstall(spec: InstallSpec) -> Result<()> {
    ensure_cargo_binstall()?;
    eprintln!(
        "greentic-dev: `{}` not found; installing `{}` via cargo binstall...",
        spec.bin_name, spec.crate_name
    );

    let status = Command::new("cargo")
        .arg("binstall")
        .arg("-y")
        .arg("--locked")
        .arg(spec.crate_name)
        .arg("--bin")
        .arg(spec.bin_name)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| "failed to execute `cargo binstall`")?;

    if status.success() {
        Ok(())
    } else {
        bail!(
            "`cargo binstall` failed while installing `{}` (crate `{}`), exit code {:?}",
            spec.bin_name,
            spec.crate_name,
            status.code()
        );
    }
}

fn ensure_cargo_binstall() -> Result<()> {
    let has_binstall = Command::new("cargo")
        .arg("binstall")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if has_binstall {
        return Ok(());
    }

    eprintln!("greentic-dev: installing `cargo-binstall` via cargo...");
    let status = Command::new("cargo")
        .arg("install")
        .arg("cargo-binstall")
        .arg("--locked")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| "failed to execute `cargo install cargo-binstall --locked`")?;

    if status.success() {
        Ok(())
    } else {
        bail!(
            "failed to install cargo-binstall; `cargo install cargo-binstall --locked` exit code {:?}",
            status.code()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{auto_install_enabled_from_env, install_spec};

    #[test]
    fn install_spec_maps_runner_cli_to_runner_crate() {
        let spec = install_spec("greentic-runner-cli").expect("runner-cli spec");
        assert_eq!(spec.crate_name, "greentic-runner");
        assert_eq!(spec.bin_name, "greentic-runner-cli");
    }

    #[test]
    fn install_spec_rejects_unknown_binary() {
        assert!(install_spec("unknown-tool").is_none());
    }

    #[test]
    fn auto_install_env_parsing() {
        assert!(auto_install_enabled_from_env(None));
        assert!(auto_install_enabled_from_env(Some("1")));
        assert!(auto_install_enabled_from_env(Some("TRUE")));
        assert!(!auto_install_enabled_from_env(Some("0")));
        assert!(!auto_install_enabled_from_env(Some("false")));
        assert!(!auto_install_enabled_from_env(Some("off")));
    }
}
