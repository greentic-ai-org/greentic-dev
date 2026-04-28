use anyhow::{Context, Result, anyhow, bail};
use semver::Version;
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use crate::toolchain_catalogue::GREENTIC_TOOLCHAIN_PACKAGES;

/// Resolve a binary by name using env override, then PATH.
pub fn resolve_binary(name: &str) -> Result<PathBuf> {
    let locale = crate::i18n::select_locale(None);
    let env_key = format!("GREENTIC_DEV_BIN_{}", name.replace('-', "_").to_uppercase());
    if let Ok(path) = env::var(&env_key) {
        let pb = PathBuf::from(path);
        if pb.exists() {
            return Ok(pb);
        }
        bail!(
            "{}",
            crate::i18n::tf(
                &locale,
                "runtime.passthrough.error.env_binary_missing",
                &[
                    ("env_key", env_key.clone()),
                    ("path", pb.display().to_string()),
                ],
            )
        );
    }

    if let Ok(path) = which::which(name) {
        return Ok(path);
    }

    bail!(
        "{}",
        crate::i18n::tf(
            &locale,
            "runtime.passthrough.error.binary_not_found",
            &[("name", name.to_string()), ("env_key", env_key)],
        )
    )
}

pub fn run_passthrough(bin: &Path, args: &[OsString], verbose: bool) -> Result<ExitStatus> {
    let locale = crate::i18n::select_locale(None);
    if verbose {
        eprintln!(
            "{}",
            crate::i18n::tf(
                &locale,
                "runtime.passthrough.debug.exec",
                &[
                    ("bin", bin.display().to_string()),
                    ("args", format!("{args:?}")),
                ],
            )
        );
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
        .map_err(|e| {
            anyhow!(crate::i18n::tf(
                &locale,
                "runtime.passthrough.error.execute",
                &[("bin", bin.display().to_string()), ("error", e.to_string())],
            ))
        })
}

pub fn install_all_delegated_tools(latest: bool, locale: &str) -> Result<()> {
    ensure_cargo_binstall()?;
    for package in GREENTIC_TOOLCHAIN_PACKAGES {
        for bin_name in package.bins {
            install_with_binstall(package.crate_name, bin_name, latest, locale)?;
        }
    }
    Ok(())
}

fn install_with_binstall(
    crate_name: &'static str,
    bin_name: &'static str,
    force_latest: bool,
    locale: &str,
) -> Result<()> {
    eprintln!(
        "{}",
        crate::i18n::tf(
            locale,
            "runtime.tools.install.installing",
            &[
                ("bin_name", bin_name.to_string()),
                ("crate_name", crate_name.to_string()),
            ],
        )
    );

    let mut cmd = Command::new("cargo");
    cmd.args(binstall_args(crate_name, bin_name, force_latest));

    let status = cmd
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| crate::i18n::t(locale, "runtime.tools.install.error.execute_binstall"))?;

    if status.success() {
        Ok(())
    } else {
        bail!(
            "{}",
            crate::i18n::tf(
                locale,
                "runtime.tools.install.error.binstall_failed",
                &[
                    ("bin_name", bin_name.to_string()),
                    ("crate_name", crate_name.to_string()),
                    ("exit_code", format!("{:?}", status.code())),
                ],
            )
        );
    }
}

fn binstall_args(
    crate_name: &'static str,
    bin_name: &'static str,
    force_latest: bool,
) -> Vec<&'static str> {
    let mut args = vec!["binstall", "-y", "--locked", crate_name, "--bin", bin_name];
    if force_latest {
        args.push("--force");
    }
    args
}

fn ensure_cargo_binstall() -> Result<()> {
    let locale = crate::i18n::select_locale(None);
    let installed_version = installed_cargo_binstall_version()?;
    if installed_version.is_none() {
        eprintln!(
            "{}",
            crate::i18n::t(&locale, "runtime.tools.install.installing_binstall")
        );
        return install_cargo_binstall();
    }

    let installed_version = installed_version.expect("checked is_some above");
    match latest_cargo_binstall_version() {
        Ok(latest_version) => {
            if installed_version >= latest_version {
                return Ok(());
            }

            eprintln!(
                "{}",
                crate::i18n::tf(
                    &locale,
                    "runtime.tools.install.updating_binstall",
                    &[
                        ("installed_version", installed_version.to_string()),
                        ("latest_version", latest_version.to_string()),
                    ],
                )
            );
            install_cargo_binstall()
        }
        Err(err) => {
            eprintln!(
                "{}",
                crate::i18n::tf(
                    &locale,
                    "runtime.tools.install.warn.latest_check_failed",
                    &[
                        ("error", err.to_string()),
                        ("installed_version", installed_version.to_string()),
                    ],
                )
            );
            Ok(())
        }
    }
}

fn install_cargo_binstall() -> Result<()> {
    let status = Command::new("cargo")
        .arg("install")
        .arg("cargo-binstall")
        .arg("--locked")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| {
            crate::i18n::t(
                &crate::i18n::select_locale(None),
                "runtime.tools.install.error.execute_install_binstall",
            )
        })?;

    if status.success() {
        Ok(())
    } else {
        let locale = crate::i18n::select_locale(None);
        bail!(
            "{}",
            crate::i18n::tf(
                &locale,
                "runtime.tools.install.error.install_binstall_failed",
                &[("exit_code", format!("{:?}", status.code()))],
            )
        );
    }
}

fn installed_cargo_binstall_version() -> Result<Option<Version>> {
    let output = Command::new("cargo")
        .arg("binstall")
        .arg("-V")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let output = match output {
        Ok(output) => output,
        Err(_) => return Ok(None),
    };
    if !output.status.success() {
        return Ok(None);
    }

    let stdout =
        String::from_utf8(output.stdout).context("`cargo binstall -V` returned non-UTF8 output")?;
    parse_installed_cargo_binstall_version(&stdout)
}

fn latest_cargo_binstall_version() -> Result<Version> {
    let output = Command::new("cargo")
        .arg("search")
        .arg("cargo-binstall")
        .arg("--limit")
        .arg("1")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .with_context(|| "failed to execute `cargo search cargo-binstall --limit 1`")?;
    if !output.status.success() {
        bail!(
            "`cargo search cargo-binstall --limit 1` failed with exit code {:?}",
            output.status.code()
        );
    }

    let stdout = String::from_utf8(output.stdout)
        .context("`cargo search cargo-binstall --limit 1` returned non-UTF8 output")?;
    parse_latest_cargo_binstall_version(&stdout)
}

fn parse_installed_cargo_binstall_version(stdout: &str) -> Result<Option<Version>> {
    let line = stdout.lines().next().unwrap_or_default();
    let maybe_version = line
        .split_whitespace()
        .find_map(|token| Version::parse(token.trim_start_matches('v')).ok());
    Ok(maybe_version)
}

fn parse_latest_cargo_binstall_version(stdout: &str) -> Result<Version> {
    let first_line = stdout
        .lines()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| anyhow!("`cargo search cargo-binstall --limit 1` returned no results"))?;
    let (_, rhs) = first_line
        .split_once('=')
        .ok_or_else(|| anyhow!("unexpected cargo search output: {first_line}"))?;
    let quoted = rhs
        .split('#')
        .next()
        .map(str::trim)
        .ok_or_else(|| anyhow!("unexpected cargo search output: {first_line}"))?;
    let version_text = quoted.trim_matches('"');
    Version::parse(version_text)
        .with_context(|| format!("failed to parse cargo-binstall version from `{first_line}`"))
}

#[cfg(test)]
mod tests {
    use super::{
        binstall_args, parse_installed_cargo_binstall_version, parse_latest_cargo_binstall_version,
    };
    use crate::toolchain_catalogue::GREENTIC_TOOLCHAIN_PACKAGES;

    #[test]
    fn delegated_install_catalogue_includes_runner() {
        let found = GREENTIC_TOOLCHAIN_PACKAGES.iter().any(|package| {
            package.crate_name == "greentic-runner" && package.bins.contains(&"greentic-runner")
        });
        assert!(found);
    }

    #[test]
    fn binstall_args_include_force_only_when_latest_requested() {
        assert_eq!(
            binstall_args("greentic-runner", "greentic-runner", false),
            vec![
                "binstall",
                "-y",
                "--locked",
                "greentic-runner",
                "--bin",
                "greentic-runner"
            ]
        );
        assert_eq!(
            binstall_args("greentic-runner", "greentic-runner", true),
            vec![
                "binstall",
                "-y",
                "--locked",
                "greentic-runner",
                "--bin",
                "greentic-runner",
                "--force"
            ]
        );
    }

    #[test]
    fn parse_installed_binstall_version_line() {
        let parsed = parse_installed_cargo_binstall_version("cargo-binstall 1.15.7\n")
            .expect("parse should succeed")
            .expect("version should exist");
        assert_eq!(parsed.to_string(), "1.15.7");
    }

    #[test]
    fn parse_latest_binstall_version_line() {
        let parsed = parse_latest_cargo_binstall_version(
            "cargo-binstall = \"1.15.7\"    # Binary installation for rust projects\n",
        )
        .expect("parse should succeed");
        assert_eq!(parsed.to_string(), "1.15.7");
    }
}
