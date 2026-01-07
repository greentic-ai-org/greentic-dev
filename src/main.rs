use anyhow::Result;
use clap::Parser;
use std::ffi::OsString;

use greentic_dev::cli::McpCommand;
use greentic_dev::cli::{Cli, Command, FlowCommand};
use greentic_dev::flow_cmd;
use greentic_dev::passthrough::{resolve_binary, run_passthrough};

use greentic_dev::cmd::config;
use greentic_dev::gui_dev::run_gui_command;
use greentic_dev::mcp_cmd;
use greentic_dev::secrets_cli::run_secrets_command;

fn main() -> Result<()> {
    let raw_args: Vec<OsString> = std::env::args_os().collect();
    let cli = Cli::parse();

    match cli.command {
        Command::Flow(flow) => match flow {
            FlowCommand::Validate(args) => flow_cmd::validate(args),
            FlowCommand::AddStep(args) => flow_cmd::run_add_step(args),
        },
        Command::Pack(_pack) => {
            let idx = raw_args
                .iter()
                .position(|a| a == "pack")
                .unwrap_or(raw_args.len().saturating_sub(1));
            let passthrough_args = &raw_args[idx + 1..];
            let subcommand = passthrough_args
                .first()
                .map(|s| s.to_string_lossy().to_string());
            let bin_name = match subcommand.as_deref() {
                Some("run") => "greentic-runner",
                Some(
                    "build" | "lint" | "components" | "update" | "new" | "sign" | "verify" | "gui"
                    | "config",
                ) => "packc",
                _ => "greentic-pack",
            };
            let bin = resolve_binary(bin_name)?;
            let args: Vec<OsString> =
                if bin_name == "greentic-runner" && matches!(subcommand.as_deref(), Some("run")) {
                    passthrough_args.iter().skip(1).cloned().collect()
                } else {
                    passthrough_args.to_vec()
                };
            let status = run_passthrough(&bin, &args, false)?;
            std::process::exit(status.code().unwrap_or(1));
        }
        Command::Component(_component) => {
            let idx = raw_args
                .iter()
                .position(|a| a == "component")
                .unwrap_or(raw_args.len().saturating_sub(1));
            let passthrough_args = &raw_args[idx + 1..];
            let bin = resolve_binary("greentic-component")?;
            let status = run_passthrough(&bin, passthrough_args, false)?;
            std::process::exit(status.code().unwrap_or(1));
        }
        Command::Config(config_cmd) => config::run(config_cmd),
        Command::Mcp(mcp) => match mcp {
            McpCommand::Doctor(args) => mcp_cmd::doctor(&args.provider, args.json),
        },
        Command::Gui(gui) => run_gui_command(gui),
        Command::Secrets(secrets) => run_secrets_command(secrets),
    }
}
