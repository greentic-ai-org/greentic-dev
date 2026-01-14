use std::{ffi::OsString, path::PathBuf};

use crate::secrets_cli::SecretsCommand;
use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "greentic-dev")]
#[command(version)]
#[command(about = "Greentic developer tooling CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Flow passthrough (greentic-flow)
    Flow(PassthroughArgs),
    /// Pack passthrough (greentic-pack; pack run uses greentic-runner-cli)
    Pack(PassthroughArgs),
    /// Component passthrough (greentic-component)
    Component(PassthroughArgs),
    /// Manage greentic-dev configuration
    #[command(subcommand)]
    Config(ConfigCommand),
    /// MCP tooling
    #[command(subcommand)]
    Mcp(McpCommand),
    /// GUI passthrough (greentic-gui)
    Gui(PassthroughArgs),
    /// Secrets convenience wrappers
    #[command(subcommand)]
    Secrets(SecretsCommand),
    /// Decode a CBOR file to text
    Cbor(CborArgs),
}

#[derive(Args, Debug, Clone)]
#[command(disable_help_flag = true)]
pub struct PassthroughArgs {
    /// Arguments passed directly to the underlying command
    #[arg(
        value_name = "ARGS",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub args: Vec<OsString>,
}

#[derive(Subcommand, Debug)]
pub enum McpCommand {
    /// Inspect MCP provider metadata
    Doctor(McpDoctorArgs),
}

#[derive(Args, Debug)]
pub struct McpDoctorArgs {
    /// MCP provider identifier or config path
    pub provider: String,
    /// Emit compact JSON instead of pretty output
    #[arg(long = "json")]
    pub json: bool,
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommand {
    /// Set a key in greentic-dev config (e.g. defaults.component.org)
    Set(ConfigSetArgs),
}

#[derive(Args, Debug)]
pub struct ConfigSetArgs {
    /// Config key path (e.g. defaults.component.org)
    pub key: String,
    /// Value to assign to the key (stored as a string)
    pub value: String,
    /// Override config file path (default: $XDG_CONFIG_HOME/greentic-dev/config.toml)
    #[arg(long = "file")]
    pub file: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct CborArgs {
    /// Path to the CBOR file to decode
    #[arg(value_name = "PATH")]
    pub path: PathBuf,
}
