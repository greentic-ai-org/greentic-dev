use std::ffi::OsString;
use std::process;

use crate::cli::{FlowAddStepArgs, FlowAddStepMode, FlowDoctorArgs};
use crate::passthrough::{resolve_binary, run_passthrough};
use crate::path_safety::normalize_under_root;
use anyhow::{Context, Result};

pub fn validate(args: FlowDoctorArgs) -> Result<()> {
    eprintln!("greentic-dev flow validate is deprecated; forwarding to greentic-flow doctor");
    doctor(args)
}

pub fn doctor(args: FlowDoctorArgs) -> Result<()> {
    let bin = resolve_binary("greentic-flow")?;
    let mut passthrough_args: Vec<OsString> = vec!["doctor".into()];
    passthrough_args.extend(args.passthrough.into_iter().map(OsString::from));
    let status = run_passthrough(&bin, &passthrough_args, false)?;
    process::exit(status.code().unwrap_or(1));
}

pub fn run_add_step(args: FlowAddStepArgs) -> Result<()> {
    let root = std::env::current_dir()
        .context("failed to resolve workspace root")?
        .canonicalize()
        .context("failed to canonicalize workspace root")?;
    let flow = normalize_under_root(&root, &args.flow_path)?;

    let bin = resolve_binary("greentic-flow")?;
    let mut passthrough_args: Vec<OsString> =
        vec!["add-step".into(), "--flow".into(), flow.into_os_string()];

    if let Some(after) = args.after {
        passthrough_args.push("--after".into());
        passthrough_args.push(after.into());
    }

    match args.mode {
        FlowAddStepMode::Default => {
            if let Some(component) = args.component_id {
                passthrough_args.push("--component".into());
                passthrough_args.push(component.into());
            }
            if let Some(component_ref) = args.component_ref {
                passthrough_args.push("--component-ref".into());
                passthrough_args.push(component_ref.into());
            }
            if let Some(local_wasm) = args.local_wasm {
                passthrough_args.push("--local-wasm".into());
                passthrough_args.push(local_wasm.into_os_string());
            }
            if args.pin {
                passthrough_args.push("--pin".into());
            }
            if let Some(alias) = args.pack_alias {
                passthrough_args.push("--pack-alias".into());
                passthrough_args.push(alias.into());
            }
            if let Some(op) = args.operation {
                passthrough_args.push("--operation".into());
                passthrough_args.push(op.into());
            }
            passthrough_args.push("--payload".into());
            passthrough_args.push(args.payload.into());
            if args.routing_out {
                passthrough_args.push("--routing-out".into());
            }
            if args.routing_reply {
                passthrough_args.push("--routing-reply".into());
            }
            if let Some(next) = args.routing_next {
                passthrough_args.push("--routing-next".into());
                passthrough_args.push(next.into());
            }
            if let Some(multi) = args.routing_multi_to {
                passthrough_args.push("--routing-multi-to".into());
                passthrough_args.push(multi.into());
            }
            if let Some(json) = args.routing_json {
                passthrough_args.push("--routing-json".into());
                passthrough_args.push(json.into_os_string());
            }
            if args.routing_to_anchor {
                passthrough_args.push("--routing-to-anchor".into());
            }
        }
        FlowAddStepMode::Config => {
            passthrough_args.push("--mode".into());
            passthrough_args.push("config".into());
            if let Some(config) = args.config_flow {
                passthrough_args.push("--config-flow".into());
                passthrough_args.push(config.into_os_string());
            }
            if let Some(answers) = args.answers {
                passthrough_args.push("--answers".into());
                passthrough_args.push(answers.into());
            }
            if let Some(file) = args.answers_file {
                passthrough_args.push("--answers-file".into());
                passthrough_args.push(file.into_os_string());
            }
        }
    }

    if args.allow_cycles {
        passthrough_args.push("--allow-cycles".into());
    }
    if args.write {
        passthrough_args.push("--write".into());
    }
    if args.validate_only {
        passthrough_args.push("--validate-only".into());
    }
    if let Some(node_id) = args.node_id {
        passthrough_args.push("--node-id".into());
        passthrough_args.push(node_id.into());
    }
    for manifest in args.manifests {
        passthrough_args.push("--manifest".into());
        passthrough_args.push(manifest.into_os_string());
    }

    let status = run_passthrough(&bin, &passthrough_args, args.verbose)?;
    process::exit(status.code().unwrap_or(1));
}
