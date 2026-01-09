use std::path::PathBuf;

use crate::secrets_cli::SecretsCommand;
use clap::{Args, Parser, Subcommand, ValueEnum};
use greentic_component::cmd::{
    build::BuildArgs as ComponentBuildArgs, doctor::DoctorArgs as ComponentDoctorArgs,
    flow::FlowCommand as ComponentFlowCommand, hash::HashArgs as ComponentHashArgs,
    inspect::InspectArgs as ComponentInspectArgs, new::NewArgs as ComponentNewArgs,
    store::StoreCommand as ComponentStoreCommand,
    templates::TemplatesArgs as ComponentTemplatesArgs,
};

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
    /// Pack tooling (delegates to greentic-pack; greentic-runner for pack run)
    #[command(subcommand)]
    Pack(PackCommand),
    /// Component tooling (delegates to greentic-component built-ins + distributor add)
    #[command(subcommand)]
    Component(ComponentCommand),
    /// Manage greentic-dev configuration
    #[command(subcommand)]
    Config(ConfigCommand),
    /// MCP tooling
    #[command(subcommand)]
    Mcp(McpCommand),
    /// GUI dev tooling (serve packs locally, stage dev packs)
    #[command(subcommand)]
    Gui(GuiCommand),
    /// Secrets convenience wrappers
    #[command(subcommand)]
    Secrets(SecretsCommand),
}

#[derive(Subcommand, Debug)]
pub enum FlowCommand {
    /// Validate a flow YAML file via greentic-flow doctor
    Validate(FlowValidateArgs),
    /// Doctor (preferred) validates a flow YAML file via greentic-flow doctor
    Doctor(FlowValidateArgs),
    /// Add a configured component step to a flow via config-flow
    AddStep(Box<FlowAddStepArgs>),
}

#[derive(Args, Debug)]
pub struct FlowValidateArgs {
    /// Path to the flow definition (YAML)
    #[arg(short = 'f', long = "file")]
    pub file: PathBuf,
    /// Emit JSON output (greentic-flow doctor --json)
    #[arg(long = "json")]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct FlowAddStepArgs {
    /// Flow file to modify (e.g., flows/main.ygtc)
    #[arg(long = "flow")]
    pub flow_path: PathBuf,
    /// Optional anchor node id; defaults to entrypoint or first node.
    #[arg(long = "after")]
    pub after: Option<String>,
    /// Mode for add-step (default or config)
    #[arg(long = "mode", value_enum, default_value = "default")]
    pub mode: FlowAddStepMode,
    /// Component id (default mode).
    #[arg(long = "component")]
    pub component_id: Option<String>,
    /// Optional pack alias for the new node.
    #[arg(long = "pack-alias")]
    pub pack_alias: Option<String>,
    /// Optional operation for the new node.
    #[arg(long = "operation")]
    pub operation: Option<String>,
    /// Payload JSON for the new node (default mode).
    #[arg(long = "payload", default_value = "{}")]
    pub payload: String,
    /// Optional routing JSON for the new node (default mode).
    #[arg(long = "routing")]
    pub routing: Option<String>,
    /// Config flow file to execute (config mode).
    #[arg(long = "config-flow")]
    pub config_flow: Option<PathBuf>,
    /// Answers JSON for config mode.
    #[arg(long = "answers")]
    pub answers: Option<String>,
    /// Answers file (JSON) for config mode.
    #[arg(long = "answers-file")]
    pub answers_file: Option<PathBuf>,
    /// Allow cycles/back-edges during insertion.
    #[arg(long = "allow-cycles")]
    pub allow_cycles: bool,
    /// Write back to the flow file instead of stdout.
    #[arg(long = "write")]
    pub write: bool,
    /// Validate only without writing output.
    #[arg(long = "validate-only")]
    pub validate_only: bool,
    /// Optional component manifest paths for catalog validation.
    #[arg(long = "manifest")]
    pub manifests: Vec<PathBuf>,
    /// Optional explicit node id hint.
    #[arg(long = "node-id")]
    pub node_id: Option<String>,
    /// Verbose passthrough logging.
    #[arg(long = "verbose")]
    pub verbose: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum FlowAddStepMode {
    Default,
    Config,
}

#[derive(Subcommand, Debug)]
pub enum GuiCommand {
    /// Serve GUI packs locally via greentic-gui
    Serve(GuiServeArgs),
    /// Stage a local GUI pack from static assets
    PackDev(GuiPackDevArgs),
}

#[derive(Args, Debug)]
pub struct GuiServeArgs {
    /// Path to gui-dev.yaml (defaults to discovery order)
    #[arg(long = "config")]
    pub config: Option<PathBuf>,
    /// Address to bind (default: 127.0.0.1:8080)
    #[arg(long = "bind")]
    pub bind: Option<String>,
    /// Domain reported to greentic-gui (default: localhost:8080)
    #[arg(long = "domain")]
    pub domain: Option<String>,
    /// Override greentic-gui binary path (otherwise PATH is used)
    #[arg(long = "gui-bin")]
    pub gui_bin: Option<PathBuf>,
    /// Disable cargo fallback when greentic-gui binary is missing
    #[arg(long = "no-cargo-fallback")]
    pub no_cargo_fallback: bool,
    /// Open a browser after the server starts
    #[arg(long = "open-browser")]
    pub open_browser: bool,
}

#[derive(Args, Debug, Clone)]
pub struct GuiPackDevArgs {
    /// Directory containing built/static assets to stage
    #[arg(long = "dir")]
    pub dir: PathBuf,
    /// Output directory for the staged pack (must be empty or absent)
    #[arg(long = "output")]
    pub output: PathBuf,
    /// Kind of GUI pack to generate (controls manifest shape)
    #[arg(long = "kind", value_enum, default_value = "layout")]
    pub kind: GuiPackKind,
    /// Entrypoint HTML file (relative to assets) for layout/feature manifests
    #[arg(long = "entrypoint", default_value = "index.html")]
    pub entrypoint: String,
    /// Optional manifest to copy instead of generating one
    #[arg(long = "manifest")]
    pub manifest: Option<PathBuf>,
    /// Feature route (only used when kind=feature)
    #[arg(long = "feature-route")]
    pub feature_route: Option<String>,
    /// Feature HTML file (relative to assets; kind=feature)
    #[arg(long = "feature-html", default_value = "index.html")]
    pub feature_html: String,
    /// Mark the feature route as authenticated (kind=feature)
    #[arg(long = "feature-authenticated")]
    pub feature_authenticated: bool,
    /// Optional build command to run before staging
    #[arg(long = "build-cmd")]
    pub build_cmd: Option<String>,
    /// Skip running the build command even if provided
    #[arg(long = "no-build")]
    pub no_build: bool,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuiPackKind {
    Layout,
    Feature,
}

#[derive(Subcommand, Debug)]
pub enum PackCommand {
    /// Delegate to greentic-pack build
    Build(PackcArgs),
    /// Delegate to greentic-pack lint
    Lint(PackcArgs),
    /// Delegate to greentic-pack components (sync pack.yaml from components/)
    Components(PackcArgs),
    /// Delegate to greentic-pack update (sync pack.yaml components + flows)
    Update(PackcArgs),
    /// Delegate to greentic-pack new
    New(PackcArgs),
    /// Delegate to greentic-pack sign
    Sign(PackcArgs),
    /// Delegate to greentic-pack verify
    Verify(PackcArgs),
    /// Delegate to greentic-pack gui helpers
    Gui(PackcArgs),
    /// Inspect a .gtpack (or directory via temporary build)
    Inspect(PackInspectArgs),
    /// Generate a deployment plan
    Plan(PackPlanArgs),
    /// Events helpers (legacy)
    #[command(subcommand)]
    Events(PackEventsCommand),
    /// Delegate to greentic-pack config (resolve config with provenance)
    Config(PackcArgs),
    /// Execute a pack locally with mocks/telemetry support
    Run(PackRunArgs),
    /// Initialize a pack workspace from a remote coordinate
    Init(PackInitArgs),
    /// Register or update a provider declaration in the pack manifest extension
    NewProvider(PackNewProviderArgs),
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
    /// Emit JSON output
    #[arg(long = "json")]
    pub json: bool,
    /// Offline mode (disable network/proxy)
    #[arg(long = "offline")]
    pub offline: bool,
    /// Use mock executor (internal/testing)
    #[arg(long = "mock-exec", hide = true)]
    pub mock_exec: bool,
    /// Allow external calls in mock executor (default: false)
    #[arg(long = "allow-external", hide = true)]
    pub allow_external: bool,
    /// Return mocked external responses when external calls are allowed (mock exec only)
    #[arg(long = "mock-external", hide = true)]
    pub mock_external: bool,
    /// Path to JSON payload used for mocked external responses (mock exec only)
    #[arg(long = "mock-external-payload", hide = true)]
    pub mock_external_payload: Option<PathBuf>,
    /// Secrets seed file applied to the mock secrets store (mock exec only)
    #[arg(long = "secrets-seed", hide = true)]
    pub secrets_seed: Option<PathBuf>,
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

#[derive(Args, Debug)]
pub struct PackInitArgs {
    /// Remote pack coordinate (e.g. pack://org/name@1.0.0)
    pub from: String,
    /// Distributor profile to use (overrides GREENTIC_DISTRIBUTOR_PROFILE/env config)
    #[arg(long = "profile")]
    pub profile: Option<String>,
}

#[derive(Args, Debug)]
pub struct PackNewProviderArgs {
    /// Path to a pack source directory, manifest.cbor, or .gtpack archive
    #[arg(long = "pack")]
    pub pack: PathBuf,
    /// Provider identifier (stored as provider_type)
    #[arg(long = "id")]
    pub id: String,
    /// Runtime reference in the form component_ref::export@world
    #[arg(long = "runtime")]
    pub runtime: String,
    /// Optional provider kind (stored in capabilities)
    #[arg(long = "kind")]
    pub kind: Option<String>,
    /// Optional external manifest/config reference (relative path)
    #[arg(long = "manifest")]
    pub manifest: Option<PathBuf>,
    /// When set, do not write changes to disk
    #[arg(long = "dry-run")]
    pub dry_run: bool,
    /// Overwrite an existing provider with the same id
    #[arg(long = "force")]
    pub force: bool,
    /// Emit JSON for the resulting provider declaration
    #[arg(long = "json")]
    pub json: bool,
    /// Scaffold provider manifest files if supported
    #[arg(long = "scaffold-files")]
    pub scaffold_files: bool,
}

#[derive(Args, Debug, Clone, Default)]
#[command(disable_help_flag = true)]
pub struct PackcArgs {
    /// Arguments passed directly to the `greentic-pack` command
    #[arg(
        value_name = "ARGS",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub passthrough: Vec<String>,
}

#[derive(Args, Debug)]
pub struct PackInspectArgs {
    /// Path to the .gtpack file or pack directory
    #[arg(value_name = "PATH")]
    pub path: PathBuf,
    /// Signature policy to enforce
    #[arg(long, value_enum, default_value = "devok")]
    pub policy: PackPolicyArg,
    /// Emit JSON output
    #[arg(long)]
    pub json: bool,
}

#[derive(Subcommand, Debug)]
pub enum PackEventsCommand {
    /// List event providers declared in a pack
    List(PackEventsListArgs),
}

#[derive(Args, Debug)]
pub struct PackEventsListArgs {
    /// Path to a .gtpack archive or pack source directory.
    #[arg(value_name = "PATH")]
    pub path: PathBuf,
    /// Output format: table (default), json, yaml.
    #[arg(long, value_enum, default_value = "table")]
    pub format: PackEventsFormatArg,
    /// When set, print additional diagnostics (for directory builds).
    #[arg(long)]
    pub verbose: bool,
}

#[derive(Args, Debug)]
pub struct PackPlanArgs {
    /// Path to a .gtpack archive or pack source directory.
    #[arg(value_name = "PATH")]
    pub input: PathBuf,
    /// Tenant identifier to embed in the plan.
    #[arg(long, default_value = "tenant-local")]
    pub tenant: String,
    /// Environment identifier to embed in the plan.
    #[arg(long, default_value = "local")]
    pub environment: String,
    /// Emit compact JSON output instead of pretty-printing.
    #[arg(long)]
    pub json: bool,
    /// When set, print additional diagnostics (for directory builds).
    #[arg(long)]
    pub verbose: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ComponentCommand {
    /// Add a remote component to the current workspace via the distributor
    Add(ComponentAddArgs),
    /// Scaffold a new Greentic component project
    New(ComponentNewArgs),
    /// List available component templates
    Templates(ComponentTemplatesArgs),
    /// Run component doctor checks
    Doctor(ComponentDoctorArgs),
    /// Inspect manifests and describe payloads
    Inspect(ComponentInspectArgs),
    /// Recompute manifest hashes
    Hash(ComponentHashArgs),
    /// Build component wasm + scaffold config flows
    Build(ComponentBuildArgs),
    /// Flow utilities (config flow scaffolding)
    #[command(subcommand)]
    Flow(ComponentFlowCommand),
    /// Interact with the component store
    #[command(subcommand)]
    Store(ComponentStoreCommand),
}

#[derive(Args, Debug, Clone)]
pub struct ComponentAddArgs {
    /// Remote component coordinate (e.g. component://org/name@^1.0)
    pub coordinate: String,
    /// Distributor profile to use (overrides GREENTIC_DISTRIBUTOR_PROFILE/env config)
    #[arg(long = "profile")]
    pub profile: Option<String>,
    /// Resolution intent (dev or runtime)
    #[arg(long = "intent", default_value = "dev", value_enum)]
    pub intent: DevIntentArg,
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

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum PackSignArg {
    Dev,
    None,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum PackPolicyArg {
    Devok,
    Strict,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum RunPolicyArg {
    Strict,
    Devok,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum VerifyPolicyArg {
    Strict,
    Devok,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum MockSettingArg {
    On,
    Off,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum PackEventsFormatArg {
    Table,
    Json,
    Yaml,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum ConfigFlowModeArg {
    Default,
    Custom,
}
#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum DevIntentArg {
    Dev,
    Runtime,
}

#[cfg(test)]
mod tests {}
