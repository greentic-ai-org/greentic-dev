use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};

use crate::i18n;
use crate::passthrough::{resolve_binary, run_passthrough};

#[derive(Subcommand, Debug)]
pub enum SecretsCommand {
    /// cli.command.secrets.init.about
    Init(SecretsInitArgs),
}

#[derive(Args, Debug, Clone)]
pub struct SecretsInitArgs {
    /// cli.command.secrets.init.pack
    #[arg(short = 'p', long = "pack")]
    pub pack: PathBuf,
    /// cli.command.secrets.init.passthrough
    #[arg(last = true)]
    pub passthrough: Vec<String>,
}

pub fn run_secrets_command(cmd: SecretsCommand, locale: &str) -> Result<()> {
    match cmd {
        SecretsCommand::Init(args) => run_init(&args, locale),
    }
}

fn run_init(args: &SecretsInitArgs, locale: &str) -> Result<()> {
    let bin = resolve_binary("greentic-secrets")?;
    let mut argv = vec![
        OsString::from("init"),
        OsString::from("--pack"),
        args.pack.clone().into_os_string(),
    ];
    argv.extend(args.passthrough.iter().map(OsString::from));
    let status = run_passthrough(&bin, &argv, false)
        .with_context(|| i18n::t(locale, "runtime.secrets.error.execute"))?;
    if !status.success() {
        bail!(
            "{}",
            i18n::tf(
                locale,
                "runtime.secrets.error.exit_status",
                &[("status", status.to_string())],
            )
        );
    }
    Ok(())
}
