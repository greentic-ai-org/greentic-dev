use std::ffi::OsString;

use anyhow::{Context, Result, anyhow, bail};
use which::which;

use crate::config::{self, GreenticConfig};
use crate::util::process::{self, CommandOutput, CommandSpec, StreamMode};

const TOOL_NAME: &str = "greentic-component";

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

    pub fn run_passthrough(&self, args: &[String]) -> Result<()> {
        let argv: Vec<OsString> = args.iter().map(OsString::from).collect();
        let label = args.first().map(|s| s.as_str()).unwrap_or("<component>");
        let output = self.exec(argv, false)?;
        self.ensure_success(label, false, &output)
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
            "`{}` {label} failed with exit code {code}",
            self.program.to_string_lossy()
        );
    }
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
