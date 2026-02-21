use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};

use crate::passthrough::{resolve_binary, run_passthrough};

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
    let bin = resolve_binary("greentic-secrets")?;
    let mut argv = vec![
        OsString::from("init"),
        OsString::from("--pack"),
        args.pack.clone().into_os_string(),
    ];
    argv.extend(args.passthrough.iter().map(OsString::from));
    let status = run_passthrough(&bin, &argv, false)
        .with_context(|| "failed to execute greentic-secrets")?;
    if !status.success() {
        bail!("greentic-secrets exited with status {}", status);
    }
    Ok(())
}
