use anyhow::{Context, Result, anyhow, bail};
use once_cell::sync::Lazy;
use semver::Version;
use serde_json::Value;
use std::ffi::OsString;
use which::which;

use crate::config::{self, GreenticConfig};
use crate::util::json;
use crate::util::process::{self, CommandOutput, CommandSpec, StreamMode};

const TOOL_NAME: &str = "greentic-component";
const MIN_COMPONENT_VERSION_STR: &str = "0.4.0";
static MIN_COMPONENT_VERSION: Lazy<Version> = Lazy::new(|| {
    Version::parse(MIN_COMPONENT_VERSION_STR).expect("valid MIN_COMPONENT_VERSION_STR")
});

#[derive(Debug, Copy, Clone)]
pub enum ComponentOperation {
    Templates,
    New,
    Validate,
    Doctor,
}

impl ComponentOperation {
    fn as_subcommand(&self) -> &'static str {
        match self {
            ComponentOperation::Templates => "templates",
            ComponentOperation::New => "new",
            ComponentOperation::Validate => "validate",
            ComponentOperation::Doctor => "doctor",
        }
    }
}

pub struct ComponentDelegate {
    program: OsString,
}

impl ComponentDelegate {
    pub fn from_config(config: &GreenticConfig) -> Result<Self> {
        let resolved = resolve_program(config)?;
        Ok(Self {
            program: resolved.program,
        })
    }

    pub fn run(
        &self,
        operation: ComponentOperation,
        mut extra_args: Vec<OsString>,
        capture_json: bool,
    ) -> Result<DelegateSuccess> {
        let mut args = Vec::with_capacity(extra_args.len() + 1);
        args.push(OsString::from(operation.as_subcommand()));
        args.append(&mut extra_args);

        let output = self.exec(args, capture_json)?;
        self.ensure_success(operation.as_subcommand(), capture_json, &output)?;

        let json = if capture_json {
            let stdout = output
                .stdout
                .as_deref()
                .context("missing stdout from component tool")?;
            Some(json::parse_json_bytes(stdout)?)
        } else {
            None
        };

        Ok(DelegateSuccess { json })
    }

    pub fn probe(&self) -> Result<ComponentProbe> {
        let version = self
            .detect_version()
            .context("failed to detect greentic-component version")?;
        if version < *MIN_COMPONENT_VERSION {
            bail!(
                "greentic-component {version} is older than required {MIN_COMPONENT_VERSION_STR}. \
                 Run `cargo install greentic-component --force`."
            );
        }

        // Run `templates --json` to ensure JSON support is available.
        self.run(
            ComponentOperation::Templates,
            vec![OsString::from("--json")],
            true,
        )
        .context("component capability probe (templates --json) failed")?;

        Ok(ComponentProbe { version })
    }

    pub fn minimum_version() -> &'static str {
        MIN_COMPONENT_VERSION_STR
    }

    fn detect_version(&self) -> Result<Version> {
        let output = self.exec(vec![OsString::from("--version")], true)?;
        self.ensure_success("--version", true, &output)?;
        let stdout = output
            .stdout
            .as_deref()
            .context("missing stdout from --version output")?;
        parse_version_from_output(stdout)
    }

    fn exec(&self, args: Vec<OsString>, capture: bool) -> Result<CommandOutput> {
        let mut spec = CommandSpec::new(self.program.clone());
        spec.args = args;
        if capture {
            spec.stdout = StreamMode::Capture;
            spec.stderr = StreamMode::Capture;
        } else {
            spec.stdout = StreamMode::Inherit;
            spec.stderr = StreamMode::Inherit;
        }
        process::run(spec)
            .with_context(|| format!("failed to spawn `{}`", self.program.to_string_lossy()))
    }

    fn ensure_success(&self, label: &str, capture: bool, output: &CommandOutput) -> Result<()> {
        if output.status.success() {
            return Ok(());
        }

        if capture
            && let Some(stderr) = output.stderr.as_ref()
            && !stderr.is_empty()
        {
            eprintln!("{}", String::from_utf8_lossy(stderr));
        }
        let code = output.status.code().unwrap_or_default();
        bail!(
            "`{}` {} failed with exit code {code}",
            self.program.to_string_lossy(),
            label
        );
    }
}

pub struct DelegateSuccess {
    pub json: Option<Value>,
}

pub struct ComponentProbe {
    pub version: Version,
}

struct ResolvedProgram {
    program: OsString,
}

fn resolve_program(config: &GreenticConfig) -> Result<ResolvedProgram> {
    if let Some(custom) = config.tools.greentic_component.path.as_ref() {
        if !custom.exists() {
            bail!(
                "configured greentic-component path `{}` does not exist",
                custom.display()
            );
        }
        return Ok(ResolvedProgram {
            program: custom.as_os_str().to_os_string(),
        });
    }

    match which(TOOL_NAME) {
        Ok(path) => Ok(ResolvedProgram {
            program: path.into_os_string(),
        }),
        Err(error) => {
            let config_hint = config::config_path()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "~/.greentic/config.toml".to_string());
            Err(anyhow!(
                "failed to locate `{TOOL_NAME}` on PATH ({error}). Install it via `cargo install \
                 greentic-component` or set [tools.greentic-component].path in {config_hint}."
            ))
        }
    }
}

fn parse_version_from_output(stdout: &[u8]) -> Result<Version> {
    let text = String::from_utf8_lossy(stdout);
    for token in text.split_whitespace() {
        if let Ok(version) = Version::parse(token) {
            return Ok(version);
        }
    }
    bail!("unable to parse greentic-component version from `{text}`");
}

#[cfg(test)]
mod tests {
    use super::parse_version_from_output;
    use semver::Version;

    #[test]
    fn parses_semver_from_version_output() {
        let version =
            parse_version_from_output(b"greentic-component 0.4.1").expect("parsed version");
        assert_eq!(version, Version::parse("0.4.1").unwrap());
    }

    #[test]
    fn rejects_invalid_output() {
        assert!(parse_version_from_output(b"???").is_err());
    }
}
