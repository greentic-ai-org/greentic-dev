use anyhow::{Context, Result, anyhow, bail};
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

/// Resolve a binary by name using env override, then PATH.
pub fn resolve_binary(name: &str) -> Result<PathBuf> {
    let env_key = format!("GREENTIC_DEV_BIN_{}", name.replace('-', "_").to_uppercase());
    if let Ok(path) = env::var(&env_key) {
        let pb = PathBuf::from(path);
        if pb.exists() {
            return Ok(pb);
        }
        bail!("{env_key} points to non-existent binary: {}", pb.display());
    }

    if let Ok(path) = which::which(name) {
        return Ok(path);
    }

    bail!(
        "failed to find `{name}` in PATH; set {env_key}, install `{name}` with cargo binstall, or run `greentic-dev install tools` (`--latest` to force-refresh)"
    )
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

const DELEGATED_INSTALL_SPECS: [InstallSpec; 7] = [
    InstallSpec {
        crate_name: "greentic-component",
        bin_name: "greentic-component",
    },
    InstallSpec {
        crate_name: "greentic-flow",
        bin_name: "greentic-flow",
    },
    InstallSpec {
        crate_name: "greentic-pack",
        bin_name: "greentic-pack",
    },
    InstallSpec {
        crate_name: "greentic-runner",
        bin_name: "greentic-runner",
    },
    InstallSpec {
        crate_name: "greentic-runner",
        bin_name: "greentic-runner-cli",
    },
    InstallSpec {
        crate_name: "greentic-gui",
        bin_name: "greentic-gui",
    },
    InstallSpec {
        crate_name: "greentic-secrets",
        bin_name: "greentic-secrets",
    },
];

pub fn install_all_delegated_tools(latest: bool) -> Result<()> {
    ensure_cargo_binstall()?;
    for spec in DELEGATED_INSTALL_SPECS {
        install_with_binstall(spec, latest)?;
    }
    Ok(())
}

fn install_with_binstall(spec: InstallSpec, force_latest: bool) -> Result<()> {
    eprintln!(
        "greentic-dev: installing `{}` from crate `{}` via cargo binstall...",
        spec.bin_name, spec.crate_name
    );

    let mut cmd = Command::new("cargo");
    cmd.arg("binstall")
        .arg("-y")
        .arg("--locked")
        .arg(spec.crate_name)
        .arg("--bin")
        .arg(spec.bin_name);
    if force_latest {
        cmd.arg("--force");
    }

    let status = cmd
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
    use super::DELEGATED_INSTALL_SPECS;

    #[test]
    fn delegated_install_specs_include_runner_cli() {
        let found = DELEGATED_INSTALL_SPECS.iter().any(|spec| {
            spec.bin_name == "greentic-runner-cli" && spec.crate_name == "greentic-runner"
        });
        assert!(found);
    }
}
