mod confirm;
mod executor;
mod persistence;
pub mod plan;
mod provider;
mod registry;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::cli::{WizardReplayArgs, WizardRunArgs};
use crate::wizard::executor::ExecuteOptions;
use crate::wizard::plan::{WizardAnswers, WizardFrontend, WizardPlan};
use crate::wizard::provider::{ProviderRequest, ShellWizardProvider, WizardProvider};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecutionMode {
    DryRun,
    Execute,
}

pub fn run(args: WizardRunArgs) -> Result<()> {
    let mode = resolve_execution_mode(args.dry_run, args.execute)?;
    let locale = args.locale.unwrap_or_else(|| "en-US".to_string());
    let frontend = WizardFrontend::parse(&args.frontend).ok_or_else(|| {
        anyhow::anyhow!(
            "unsupported frontend `{}`; expected text|json|adaptive-card",
            args.frontend
        )
    })?;

    if registry::resolve(&args.target, &args.mode).is_none() {
        bail!(
            "unsupported wizard target/mode `{}`.`{}` for PR-01",
            args.target,
            args.mode
        );
    }

    let answers_file = load_answers_file(args.answers.as_deref())?;
    let merged_answers = merge_answers(None, None, Some(answers_file), None);
    let provider = ShellWizardProvider;
    let req = ProviderRequest {
        target: args.target.clone(),
        mode: args.mode.clone(),
        frontend: frontend.clone(),
        locale: locale.clone(),
        answers: merged_answers.clone(),
    };
    let mut plan = provider.build_plan(&req)?;

    let out_dir = persistence::resolve_out_dir(args.out.as_deref());
    let paths = persistence::prepare_dir(&out_dir)?;
    persistence::persist_plan_and_answers(&paths, &merged_answers, &plan)?;

    render_plan(&plan)?;

    if mode == ExecutionMode::Execute {
        confirm::ensure_execute_allowed(
            &format!(
                "Plan `{}`.`{}` with {} step(s)",
                args.target,
                args.mode,
                plan.steps.len()
            ),
            args.yes,
            args.non_interactive,
        )?;
        let report = executor::execute(
            &plan,
            &paths.exec_log_path,
            &ExecuteOptions {
                unsafe_commands: args.unsafe_commands,
                allow_destructive: args.allow_destructive,
            },
        )?;
        annotate_execution_metadata(&mut plan, &report);
        persistence::persist_plan_and_answers(&paths, &merged_answers, &plan)?;
    }

    Ok(())
}

pub fn replay(args: WizardReplayArgs) -> Result<()> {
    let mode = resolve_execution_mode(args.dry_run, args.execute)?;
    let (answers, mut plan, replay_root) = persistence::load_replay(&args.answers)?;
    let out_dir = args.out.unwrap_or(replay_root);
    let paths = persistence::prepare_dir(&out_dir)?;
    persistence::persist_plan_and_answers(&paths, &answers, &plan)?;
    render_plan(&plan)?;

    if mode == ExecutionMode::Execute {
        confirm::ensure_execute_allowed(
            &format!(
                "Replay plan `{}`.`{}` with {} step(s)",
                plan.metadata.target,
                plan.metadata.mode,
                plan.steps.len()
            ),
            args.yes,
            args.non_interactive,
        )?;
        let report = executor::execute(
            &plan,
            &paths.exec_log_path,
            &ExecuteOptions {
                unsafe_commands: args.unsafe_commands,
                allow_destructive: args.allow_destructive,
            },
        )?;
        annotate_execution_metadata(&mut plan, &report);
        persistence::persist_plan_and_answers(&paths, &answers, &plan)?;
    }
    Ok(())
}

fn render_plan(plan: &WizardPlan) -> Result<()> {
    let rendered = match plan.metadata.frontend {
        WizardFrontend::Json => {
            serde_json::to_string_pretty(plan).context("failed to encode wizard plan")?
        }
        WizardFrontend::Text => render_text_plan(plan),
        WizardFrontend::AdaptiveCard => {
            let card = serde_json::json!({
                "type": "AdaptiveCard",
                "version": "1.5",
                "body": [
                    {"type":"TextBlock","weight":"Bolder","text":"greentic-dev wizard plan"},
                    {"type":"TextBlock","text": format!("target: {} mode: {}", plan.metadata.target, plan.metadata.mode)},
                ],
                "data": { "plan": plan }
            });
            serde_json::to_string_pretty(&card).context("failed to encode adaptive card")?
        }
    };
    println!("{rendered}");
    Ok(())
}

fn render_text_plan(plan: &WizardPlan) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "wizard plan v{}: {}.{}\n",
        plan.plan_version, plan.metadata.target, plan.metadata.mode
    ));
    out.push_str(&format!("locale: {}\n", plan.metadata.locale));
    out.push_str(&format!("steps: {}\n", plan.steps.len()));
    for (idx, step) in plan.steps.iter().enumerate() {
        match step {
            crate::wizard::plan::WizardStep::RunCommand(cmd) => {
                out.push_str(&format!(
                    "{}. RunCommand {} {}\n",
                    idx + 1,
                    cmd.program,
                    cmd.args.join(" ")
                ));
            }
            other => {
                out.push_str(&format!("{}. {:?}\n", idx + 1, other));
            }
        }
    }
    out
}

fn load_answers_file(path: Option<&Path>) -> Result<serde_json::Value> {
    let Some(path) = path else {
        return Ok(serde_json::Value::Object(Default::default()));
    };
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(value)
}

fn merge_answers(
    cli_overrides: Option<serde_json::Value>,
    parent_prefill: Option<serde_json::Value>,
    answers_file: Option<serde_json::Value>,
    provider_defaults: Option<serde_json::Value>,
) -> WizardAnswers {
    // Highest -> lowest: CLI overrides, parent prefill, answers file, provider defaults.
    let mut out = BTreeMap::<String, serde_json::Value>::new();
    merge_obj(&mut out, provider_defaults);
    merge_obj(&mut out, answers_file);
    merge_obj(&mut out, parent_prefill);
    merge_obj(&mut out, cli_overrides);
    WizardAnswers {
        data: serde_json::Value::Object(out.into_iter().collect()),
    }
}

fn merge_obj(dst: &mut BTreeMap<String, serde_json::Value>, src: Option<serde_json::Value>) {
    if let Some(serde_json::Value::Object(map)) = src {
        for (k, v) in map {
            dst.insert(k, v);
        }
    }
}

fn resolve_execution_mode(dry_run: bool, execute: bool) -> Result<ExecutionMode> {
    if dry_run && execute {
        bail!("Choose one of --dry-run or --execute.");
    }
    if execute {
        Ok(ExecutionMode::Execute)
    } else {
        Ok(ExecutionMode::DryRun)
    }
}

fn annotate_execution_metadata(
    plan: &mut WizardPlan,
    report: &crate::wizard::executor::ExecutionReport,
) {
    for (program, version) in &report.resolved_versions {
        plan.inputs
            .insert(format!("resolved_versions.{program}"), version.clone());
    }
    plan.inputs.insert(
        "executed_commands".to_string(),
        report.commands_executed.to_string(),
    );
}

#[cfg(test)]
mod tests {
    use super::{ExecutionMode, merge_answers, resolve_execution_mode};
    use serde_json::json;

    #[test]
    fn mode_defaults_to_dry_run() {
        let mode = resolve_execution_mode(false, false).unwrap();
        assert_eq!(mode, ExecutionMode::DryRun);
    }

    #[test]
    fn mode_rejects_both_flags() {
        let err = resolve_execution_mode(true, true).unwrap_err().to_string();
        assert!(err.contains("Choose one of --dry-run or --execute."));
    }

    #[test]
    fn answer_precedence_parent_over_file() {
        let merged = merge_answers(
            None,
            Some(json!({"foo":"parent","bar":"parent"})),
            Some(json!({"foo":"file","baz":"file"})),
            None,
        );
        assert_eq!(merged.data["foo"], "parent");
        assert_eq!(merged.data["bar"], "parent");
        assert_eq!(merged.data["baz"], "file");
    }

    #[test]
    fn answer_precedence_cli_over_parent() {
        let merged = merge_answers(
            Some(json!({"foo":"cli"})),
            Some(json!({"foo":"parent"})),
            Some(json!({"foo":"file"})),
            None,
        );
        assert_eq!(merged.data["foo"], "cli");
    }

    #[test]
    fn answer_precedence_file_over_defaults() {
        let merged = merge_answers(
            None,
            None,
            Some(json!({"foo":"file"})),
            Some(json!({"foo":"default","bar":"default"})),
        );
        assert_eq!(merged.data["foo"], "file");
        assert_eq!(merged.data["bar"], "default");
    }
}
