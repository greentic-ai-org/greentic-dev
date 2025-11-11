use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

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
    /// Flow tooling (validate, lint, bundle inspection)
    #[command(subcommand)]
    Flow(FlowCommand),
    /// Pack tooling (build deterministic packs, run locally)
    #[command(subcommand)]
    Pack(PackCommand),
    /// Component tooling (delegates to `greentic-component`)
    Component(ComponentPassthroughArgs),
    /// Manage greentic-dev configuration
    #[command(subcommand)]
    Config(ConfigCommand),
    /// MCP tooling (feature = "mcp")
    #[cfg(feature = "mcp")]
    #[command(subcommand)]
    Mcp(McpCommand),
}

#[derive(Subcommand, Debug)]
pub enum FlowCommand {
    /// Validate a flow YAML file and emit the canonical bundle JSON
    Validate(FlowValidateArgs),
}

#[derive(Args, Debug)]
pub struct FlowValidateArgs {
    /// Path to the flow definition (YAML)
    #[arg(short = 'f', long = "file")]
    pub file: PathBuf,
    /// Emit compact JSON instead of pretty-printing
    #[arg(long = "json")]
    pub json: bool,
}

#[derive(Subcommand, Debug)]
pub enum PackCommand {
    /// Build a deterministic .gtpack from a validated flow bundle
    Build(PackBuildArgs),
    /// Execute a pack locally with mocks/telemetry support
    Run(PackRunArgs),
    /// Scaffold a pack workspace via the `packc` CLI
    New(PackNewArgs),
}

#[derive(Args, Debug)]
pub struct PackBuildArgs {
    /// Path to the flow definition (YAML)
    #[arg(short = 'f', long = "file")]
    pub file: PathBuf,
    /// Output path for the generated pack
    #[arg(short = 'o', long = "out")]
    pub out: PathBuf,
    /// Signing mode for the generated pack
    #[arg(long = "sign", default_value = "dev", value_enum)]
    pub sign: PackSignArg,
    /// Optional path to pack metadata (pack.toml)
    #[arg(long = "meta")]
    pub meta: Option<PathBuf>,
    /// Directory containing local component builds
    #[arg(long = "component-dir", value_name = "DIR")]
    pub component_dir: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct PackRunArgs {
    /// Path to the pack (.gtpack) to execute
    #[arg(short = 'p', long = "pack")]
    pub pack: PathBuf,
    /// Flow entry identifier override
    #[arg(long = "entry")]
    pub entry: Option<String>,
    /// JSON payload to use as run input
    #[arg(long = "input")]
    pub input: Option<String>,
    /// Enforcement policy for pack signatures
    #[arg(long = "policy", default_value = "devok", value_enum)]
    pub policy: RunPolicyArg,
    /// OTLP collector endpoint (optional)
    #[arg(long = "otlp")]
    pub otlp: Option<String>,
    /// Comma-separated list of allowed outbound hosts
    #[arg(long = "allow")]
    pub allow: Option<String>,
    /// Mocks toggle
    #[arg(long = "mocks", default_value = "on", value_enum)]
    pub mocks: MockSettingArg,
    /// Directory to persist run artifacts (transcripts, logs)
    #[arg(long = "artifacts")]
    pub artifacts: Option<PathBuf>,
}

#[derive(Args, Debug, Clone, Default)]
pub struct PackNewArgs {
    /// Arguments passed directly to the `packc new` command
    #[arg(
        value_name = "ARGS",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub passthrough: Vec<String>,
}

#[derive(Args, Debug, Clone, Default)]
pub struct ComponentPassthroughArgs {
    /// Arguments passed directly to the `greentic-component` CLI
    #[arg(
        value_name = "ARGS",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub passthrough: Vec<String>,
}

#[cfg(feature = "mcp")]
#[derive(Subcommand, Debug)]
pub enum McpCommand {
    /// Inspect MCP provider metadata
    Doctor(McpDoctorArgs),
}

#[cfg(feature = "mcp")]
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
    /// Set a key in ~/.greentic/config.toml (e.g. defaults.component.org)
    Set(ConfigSetArgs),
}

#[derive(Args, Debug)]
pub struct ConfigSetArgs {
    /// Config key path (e.g. defaults.component.org)
    pub key: String,
    /// Value to assign to the key (stored as a string)
    pub value: String,
    /// Override config file path (default: ~/.greentic/config.toml)
    #[arg(long = "file")]
    pub file: Option<PathBuf>,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum PackSignArg {
    Dev,
    None,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum RunPolicyArg {
    Strict,
    Devok,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum MockSettingArg {
    On,
    Off,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_component_passthrough_args() {
        let cli = Cli::parse_from([
            "greentic-dev",
            "component",
            "new",
            "--name",
            "demo",
            "--json",
        ]);
        let Command::Component(args) = cli.command else {
            panic!("expected component passthrough variant");
        };
        assert_eq!(
            args.passthrough,
            vec![
                "new".to_string(),
                "--name".into(),
                "demo".into(),
                "--json".into()
            ]
        );
    }

    #[test]
    fn parses_pack_new_args() {
        let cli = Cli::parse_from(["greentic-dev", "pack", "new", "--name", "demo-pack"]);
        let Command::Pack(PackCommand::New(args)) = cli.command else {
            panic!("expected pack new variant");
        };
        assert_eq!(
            args.passthrough,
            vec!["--name".to_string(), "demo-pack".to_string()]
        );
    }

    #[test]
    fn parses_config_set_command() {
        let cli = Cli::parse_from([
            "greentic-dev",
            "config",
            "set",
            "defaults.component.org",
            "ai.greentic",
            "--file",
            "/tmp/config.toml",
        ]);
        let Command::Config(ConfigCommand::Set(args)) = cli.command else {
            panic!("expected config set variant");
        };
        assert_eq!(args.key, "defaults.component.org");
        assert_eq!(args.value, "ai.greentic");
        assert_eq!(
            args.file.as_ref().map(|p| p.display().to_string()),
            Some("/tmp/config.toml".into())
        );
    }
}
