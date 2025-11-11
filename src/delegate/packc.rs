use std::ffi::OsString;

use anyhow::{Context, Result, anyhow, bail};
use which::which;

use crate::config::GreenticConfig;
use crate::util::process::{self, CommandOutput, CommandSpec, StreamMode};

const TOOL_NAME: &str = "packc";

pub struct PackcDelegate {
    program: OsString,
}

impl PackcDelegate {
    pub fn from_config(config: &GreenticConfig) -> Result<Self> {
        let resolved = resolve_program(config)?;
        Ok(Self {
            program: resolved.program,
        })
    }

    pub fn run_new(&self, args: &[String]) -> Result<()> {
        let mut argv = Vec::with_capacity(args.len() + 1);
        argv.push(OsString::from("new"));
        argv.extend(args.iter().map(OsString::from));
        let output = self.exec(argv)?;
        self.ensure_success("new", &output)
    }

    fn exec(&self, args: Vec<OsString>) -> Result<CommandOutput> {
        let mut spec = CommandSpec::new(self.program.clone());
        spec.args = args;
        spec.stdout = StreamMode::Inherit;
        spec.stderr = StreamMode::Inherit;
        process::run(spec)
            .with_context(|| format!("failed to spawn `{}`", self.program.to_string_lossy()))
    }

    fn ensure_success(&self, label: &str, output: &CommandOutput) -> Result<()> {
        if output.status.success() {
            return Ok(());
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
    if let Some(custom) = config.tools.packc.path.as_ref() {
        if !custom.exists() {
            bail!(
                "configured packc path `{}` does not exist",
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
        Err(error) => Err(anyhow!(
            "failed to locate `{TOOL_NAME}` on PATH ({error}). Install it via `cargo install \
                 greentic-pack --bin packc` or set [tools.packc].path in ~/.greentic/config.toml."
        )),
    }
}
