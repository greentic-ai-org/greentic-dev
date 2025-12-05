mod cmd;
mod component_resolver;
mod config;
mod delegate;
mod flow_cmd;
#[cfg(feature = "mcp")]
mod mcp_cmd;
mod pack_build;
mod pack_run;
mod pack_verify;
mod path_safety;
mod util;

use anyhow::Result;
use clap::Parser;
#[cfg(feature = "mcp")]
use greentic_dev::cli::McpCommand;
use greentic_dev::cli::{
    Cli, Command, FlowCommand, MockSettingArg, PackCommand, PackSignArg, RunPolicyArg,
    VerifyPolicyArg,
};

use crate::pack_build::PackSigning;
use crate::pack_run::{MockSetting, RunPolicy};
use crate::pack_verify::VerifyPolicy;

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Flow(flow) => match flow {
            FlowCommand::Validate(args) => flow_cmd::validate(&args.file, args.json),
        },
        Command::Pack(pack) => match pack {
            PackCommand::Build(args) => pack_build::run(
                &args.file,
                &args.out,
                args.sign.into(),
                args.meta.as_deref(),
                args.component_dir.as_deref(),
            ),
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
                    policy: args.policy.into(),
                    otlp: args.otlp,
                    allow_hosts,
                    mocks: args.mocks.into(),
                    artifacts_dir: args.artifacts.as_deref(),
                })
            }
            PackCommand::Verify(args) => {
                pack_verify::run(&args.pack, args.policy.into(), args.json)
            }
            PackCommand::New(args) => cmd::pack::run_new(&args),
        },
        Command::Component(component) => cmd::component::run_passthrough(&component),
        Command::Config(config_cmd) => cmd::config::run(config_cmd),
        #[cfg(feature = "mcp")]
        Command::Mcp(mcp) => match mcp {
            McpCommand::Doctor(args) => mcp_cmd::doctor(&args.provider, args.json),
        },
    }
}

impl From<PackSignArg> for PackSigning {
    fn from(value: PackSignArg) -> Self {
        match value {
            PackSignArg::Dev => PackSigning::Dev,
            PackSignArg::None => PackSigning::None,
        }
    }
}

impl From<RunPolicyArg> for RunPolicy {
    fn from(value: RunPolicyArg) -> Self {
        match value {
            RunPolicyArg::Strict => RunPolicy::Strict,
            RunPolicyArg::Devok => RunPolicy::DevOk,
        }
    }
}

impl From<MockSettingArg> for MockSetting {
    fn from(value: MockSettingArg) -> Self {
        match value {
            MockSettingArg::On => MockSetting::On,
            MockSettingArg::Off => MockSetting::Off,
        }
    }
}

impl From<VerifyPolicyArg> for VerifyPolicy {
    fn from(value: VerifyPolicyArg) -> Self {
        match value {
            VerifyPolicyArg::Strict => VerifyPolicy::Strict,
            VerifyPolicyArg::Devok => VerifyPolicy::DevOk,
        }
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
