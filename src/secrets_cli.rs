use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};

#[derive(Subcommand, Debug)]
pub enum SecretsCommand {
    /// Delegate to greentic-secrets to initialize secrets for a pack
    Init(SecretsInitArgs),
}

#[derive(Args, Debug, Clone)]
pub struct SecretsInitArgs {
    /// Path to the pack (.gtpack) to initialize
    #[arg(short = 'p', long = "pack")]
    pub pack: PathBuf,
    /// Optional extra args passed through to greentic-secrets (add `--` before flags)
    #[arg(last = true)]
    pub passthrough: Vec<String>,
}

pub fn run_secrets_command(cmd: SecretsCommand) -> Result<()> {
    match cmd {
        SecretsCommand::Init(args) => run_init(&args),
    }
}

fn run_init(args: &SecretsInitArgs) -> Result<()> {
    let mut command = Command::new("greentic-secrets");
    command
        .arg("init")
        .arg("--pack")
        .arg(&args.pack)
        .args(&args.passthrough)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let status = command
        .status()
        .with_context(|| "failed to execute greentic-secrets (is it on PATH?)")?;
    if !status.success() {
        bail!("greentic-secrets exited with status {}", status);
    }
    Ok(())
}
