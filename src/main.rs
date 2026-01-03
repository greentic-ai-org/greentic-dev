use anyhow::{Context, Result};
use clap::Parser;
use greentic_dev::cli::McpCommand;
use greentic_dev::cli::{
    Cli, Command, ComponentCommand, DevIntentArg, FlowCommand, MockSettingArg, PackCommand,
    PackEventsCommand, RunPolicyArg,
};
use greentic_dev::flow_cmd;
use greentic_dev::pack_init::{PackInitIntent, run as pack_init_run};

use greentic_component::cmd::{build, doctor, flow, hash, inspect, new, store, templates};
use greentic_component::scaffold::engine::ScaffoldEngine;
use greentic_dev::cmd;
use greentic_dev::component_add::run_component_add;
use greentic_dev::gui_dev::run_gui_command;
use greentic_dev::mcp_cmd;
use greentic_dev::pack_cli;
use greentic_dev::pack_cli::{pack_inspect, pack_plan};
use greentic_dev::pack_run::{self, MockSetting, RunPolicy};
use greentic_dev::secrets_cli::run_secrets_command;
use packc::cli as packc_cli;

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Flow(flow) => match flow {
            FlowCommand::Validate(args) => flow_cmd::validate(&args.file, args.json),
            FlowCommand::AddStep(args) => flow_cmd::run_add_step(args),
        },
        Command::Pack(pack) => match pack {
            PackCommand::Build(args) => packc("build", &args.passthrough),
            PackCommand::Lint(args) => packc("lint", &args.passthrough),
            PackCommand::Components(args) => packc("components", &args.passthrough),
            PackCommand::Update(args) => packc("update", &args.passthrough),
            PackCommand::New(args) => packc("new", &args.passthrough),
            PackCommand::Sign(args) => packc("sign", &args.passthrough),
            PackCommand::Verify(args) => packc("verify", &args.passthrough),
            PackCommand::Gui(args) => packc("gui", &args.passthrough),
            PackCommand::Inspect(args) => pack_inspect(&args.path, args.policy, args.json),
            PackCommand::Plan(args) => pack_plan(&args),
            PackCommand::Events(evt) => match evt {
                PackEventsCommand::List(args) => pack_cli::pack_events_list(&args),
            },
            PackCommand::Config(args) => packc("config", &args.passthrough),
            PackCommand::Run(args) => {
                let allow_hosts = args
                    .allow
                    .as_ref()
                    .map(|value| split_allow_list(value))
                    .transpose()?;
                pack_run::run(pack_run::PackRunConfig {
                    pack_path: &args.pack,
                    entry: args.entry,
                    input: args.input,
                    policy: run_policy_from_arg(args.policy),
                    otlp: args.otlp,
                    allow_hosts,
                    mocks: mock_setting_from_arg(args.mocks),
                    artifacts_dir: args.artifacts.as_deref(),
                    json: args.json,
                    offline: args.offline,
                    mock_exec: args.mock_exec,
                    allow_external: args.allow_external,
                    mock_external: args.mock_external,
                    mock_external_payload: args
                        .mock_external_payload
                        .as_ref()
                        .map(|p| -> anyhow::Result<_> {
                            let data = std::fs::read_to_string(p)
                                .with_context(|| format!("failed to read {}", p.display()))?;
                            serde_json::from_str(&data).context("invalid mock external JSON")
                        })
                        .transpose()?,
                    secrets_seed: args.secrets_seed.as_deref(),
                })
            }
            PackCommand::Init(args) => pack_init_run(&args.from, args.profile.as_deref()),
            PackCommand::NewProvider(args) => pack_cli::pack_new_provider(&args),
        },
        Command::Component(component) => match component {
            ComponentCommand::Add(args) => {
                let _ = run_component_add(
                    &args.coordinate,
                    args.profile.as_deref(),
                    match args.intent {
                        DevIntentArg::Dev => PackInitIntent::Dev,
                        DevIntentArg::Runtime => PackInitIntent::Runtime,
                    },
                )?;
                Ok(())
            }
            ComponentCommand::New(args) => {
                let engine = ScaffoldEngine::new();
                new::run(args, &engine)
            }
            ComponentCommand::Templates(args) => {
                let engine = ScaffoldEngine::new();
                templates::run(args, &engine)
            }
            ComponentCommand::Doctor(args) => doctor::run(args).map_err(Into::into),
            ComponentCommand::Inspect(args) => {
                let result = inspect::run(&args)?;
                inspect::emit_warnings(&result.warnings);
                if args.strict && !result.warnings.is_empty() {
                    anyhow::bail!(
                        "component-inspect: {} warning(s) treated as errors (--strict)",
                        result.warnings.len()
                    );
                }
                Ok(())
            }
            ComponentCommand::Hash(args) => hash::run(args),
            ComponentCommand::Build(args) => build::run(args),
            ComponentCommand::Flow(flow) => flow::run(flow),
            ComponentCommand::Store(store) => store::run(store),
        },
        Command::Config(config_cmd) => cmd::config::run(config_cmd),
        Command::Mcp(mcp) => match mcp {
            McpCommand::Doctor(args) => mcp_cmd::doctor(&args.provider, args.json),
        },
        Command::Gui(gui) => run_gui_command(gui),
        Command::Secrets(secrets) => run_secrets_command(secrets),
    }
}

fn split_allow_list(value: &str) -> Result<Vec<String>> {
    let hosts = value
        .split(',')
        .map(|segment| segment.trim())
        .filter(|segment| !segment.is_empty())
        .map(|segment| segment.to_string())
        .collect::<Vec<_>>();
    if hosts.is_empty() {
        anyhow::bail!("--allow expects at least one host when provided");
    }
    Ok(hosts)
}

fn packc(subcommand: &str, args: &[String]) -> Result<()> {
    let mut argv = Vec::with_capacity(args.len() + 2);
    argv.push("packc".to_string());
    argv.push(subcommand.to_string());
    argv.extend(args.iter().cloned());
    let cli = packc_cli::Cli::parse_from(argv);
    packc_cli::run_with_cli(cli)?;
    Ok(())
}

fn run_policy_from_arg(arg: RunPolicyArg) -> RunPolicy {
    match arg {
        RunPolicyArg::Strict => RunPolicy::Strict,
        RunPolicyArg::Devok => RunPolicy::DevOk,
    }
}

fn mock_setting_from_arg(arg: MockSettingArg) -> MockSetting {
    match arg {
        MockSettingArg::On => MockSetting::On,
        MockSettingArg::Off => MockSetting::Off,
    }
}
