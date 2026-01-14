use anyhow::Result;
use clap::Parser;

use greentic_dev::cli::McpCommand;
use greentic_dev::cli::{Cli, Command};
use greentic_dev::passthrough::{resolve_binary, run_passthrough};

use greentic_dev::cbor_cmd;
use greentic_dev::cmd::config;
use greentic_dev::mcp_cmd;
use greentic_dev::secrets_cli::run_secrets_command;

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Flow(args) => {
            let bin = resolve_binary("greentic-flow")?;
            let status = run_passthrough(&bin, &args.args, false)?;
            std::process::exit(status.code().unwrap_or(1));
        }
        Command::Pack(args) => {
            let subcommand = args.args.first().and_then(|s| s.to_str());
            if subcommand == Some("run") {
                let bin = resolve_binary("greentic-runner-cli")?;
                let run_args = &args.args[1..];
                let status = run_passthrough(&bin, run_args, false)?;
                std::process::exit(status.code().unwrap_or(1));
            }

            let bin = resolve_binary("greentic-pack")?;
            let status = run_passthrough(&bin, &args.args, false)?;
            std::process::exit(status.code().unwrap_or(1));
        }
        Command::Component(args) => {
            let bin = resolve_binary("greentic-component")?;
            let status = run_passthrough(&bin, &args.args, false)?;
            std::process::exit(status.code().unwrap_or(1));
        }
        Command::Config(config_cmd) => config::run(config_cmd),
        Command::Cbor(args) => cbor_cmd::run(args),
        Command::Mcp(mcp) => match mcp {
            McpCommand::Doctor(args) => mcp_cmd::doctor(&args.provider, args.json),
        },
        Command::Gui(args) => {
            let bin = resolve_binary("greentic-gui")?;
            let status = run_passthrough(&bin, &args.args, false)?;
            std::process::exit(status.code().unwrap_or(1));
        }
        Command::Secrets(secrets) => run_secrets_command(secrets),
    }
}
