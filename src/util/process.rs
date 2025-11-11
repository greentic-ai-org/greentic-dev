use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};

use anyhow::{Context, Result};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamMode {
    Inherit,
    Capture,
}

pub struct CommandSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
    pub current_dir: Option<PathBuf>,
    pub stdout: StreamMode,
    pub stderr: StreamMode,
}

impl CommandSpec {
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
            current_dir: None,
            stdout: StreamMode::Inherit,
            stderr: StreamMode::Inherit,
        }
    }
}

pub struct CommandOutput {
    pub status: ExitStatus,
    #[allow(dead_code)]
    pub stdout: Option<Vec<u8>>,
    pub stderr: Option<Vec<u8>>,
}

pub fn run(spec: CommandSpec) -> Result<CommandOutput> {
    let mut command = Command::new(&spec.program);
    command.args(&spec.args);
    if let Some(dir) = &spec.current_dir {
        command.current_dir(dir);
    }
    for (key, value) in &spec.env {
        command.env(key, value);
    }

    match (spec.stdout, spec.stderr) {
        (StreamMode::Inherit, StreamMode::Inherit) => {
            command.stdout(Stdio::inherit());
            command.stderr(Stdio::inherit());
            let status = command
                .status()
                .with_context(|| format!("failed to spawn `{}`", spec.program.to_string_lossy()))?;
            Ok(CommandOutput {
                status,
                stdout: None,
                stderr: None,
            })
        }
        (StreamMode::Capture, StreamMode::Capture) => {
            command.stdout(Stdio::piped());
            command.stderr(Stdio::piped());
            let output = command
                .output()
                .with_context(|| format!("failed to spawn `{}`", spec.program.to_string_lossy()))?;
            Ok(CommandOutput {
                status: output.status,
                stdout: Some(output.stdout),
                stderr: Some(output.stderr),
            })
        }
        _ => anyhow::bail!("mixed capture/inherit mode is not supported yet"),
    }
}
