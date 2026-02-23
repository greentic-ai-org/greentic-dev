use std::collections::BTreeMap;

use anyhow::{Result, bail};

use crate::wizard::plan::{
    RunCommandStep, WizardAnswers, WizardFrontend, WizardPlan, WizardPlanMetadata, WizardStep,
};

pub trait WizardProvider {
    fn build_plan(&self, req: &ProviderRequest) -> Result<WizardPlan>;
}

#[derive(Debug, Clone)]
pub struct ProviderRequest {
    pub target: String,
    pub mode: String,
    pub frontend: WizardFrontend,
    pub locale: String,
    pub answers: WizardAnswers,
}

pub struct ShellWizardProvider;

impl WizardProvider for ShellWizardProvider {
    fn build_plan(&self, req: &ProviderRequest) -> Result<WizardPlan> {
        let mut steps = high_level_steps(&req.target, &req.mode)?;
        steps.push(WizardStep::RunCommand(command_for_key(
            &req.target,
            &req.mode,
            &req.answers,
        )?));

        let mut inputs = BTreeMap::new();
        if req.answers.data != serde_json::Value::Object(Default::default()) {
            inputs.insert("answers_ref".to_string(), "answers.json".to_string());
        }
        for (k, v) in extract_provider_refs(&req.answers) {
            inputs.insert(format!("provider_refs.{k}"), v);
        }

        Ok(WizardPlan {
            plan_version: 1,
            created_at: None,
            metadata: WizardPlanMetadata {
                target: req.target.clone(),
                mode: req.mode.clone(),
                locale: req.locale.clone(),
                frontend: req.frontend.clone(),
            },
            inputs,
            steps,
        })
    }
}

fn high_level_steps(target: &str, mode: &str) -> Result<Vec<WizardStep>> {
    let steps = match (target, mode) {
        ("operator", "create") => vec![WizardStep::CreateBundle],
        ("pack", "create") => vec![WizardStep::CreateGtpack],
        ("pack", "build") => vec![WizardStep::ResolvePacks],
        ("component", "scaffold") => vec![WizardStep::ScaffoldComponent],
        ("component", "build") => vec![WizardStep::BuildComponent],
        ("flow", "create") => vec![WizardStep::CreateFlow],
        ("flow", "wire") => vec![WizardStep::WireFlow],
        ("bundle", "create") => vec![WizardStep::CreateBundle, WizardStep::AddPacksToBundle],
        ("dev", "doctor") => vec![WizardStep::RunDoctor],
        ("dev", "run") => vec![WizardStep::DevRun],
        _ => bail!("unsupported wizard mapping for `{target}.{mode}`"),
    };
    Ok(steps)
}

fn command_for_key(target: &str, mode: &str, answers: &WizardAnswers) -> Result<RunCommandStep> {
    let step = match (target, mode) {
        ("operator", "create") => RunCommandStep {
            program: "greentic-operator".to_string(),
            args: build_args_with_fallback(
                vec!["bundle".to_string(), "create".to_string()],
                answers,
                &[("name", "--name"), ("out", "--out")],
                vec![
                    "bundle".to_string(),
                    "create".to_string(),
                    "--help".to_string(),
                ],
            ),
            destructive: false,
        },
        ("pack", "create") => RunCommandStep {
            program: "greentic-pack".to_string(),
            args: build_args_with_fallback(
                vec!["new".to_string()],
                answers,
                &[("dir", "--dir"), ("name", "--name"), ("pack_id", "--id")],
                vec!["new".to_string(), "--help".to_string()],
            ),
            destructive: false,
        },
        ("pack", "build") => RunCommandStep {
            program: "greentic-pack".to_string(),
            args: build_args(
                vec!["build".to_string()],
                answers,
                &[("in", "--in"), ("gtpack_out", "--gtpack-out")],
            ),
            destructive: false,
        },
        ("component", "scaffold") => RunCommandStep {
            program: "greentic-component".to_string(),
            args: build_args_with_fallback(
                vec!["new".to_string()],
                answers,
                &[
                    ("name", "--name"),
                    ("path", "--path"),
                    ("template", "--template"),
                ],
                vec!["new".to_string(), "--help".to_string()],
            ),
            destructive: false,
        },
        ("component", "build") => RunCommandStep {
            program: "greentic-component".to_string(),
            args: build_args_with_fallback(
                vec!["build".to_string()],
                answers,
                &[("manifest", "--manifest")],
                vec!["build".to_string(), "--help".to_string()],
            ),
            destructive: false,
        },
        ("flow", "create") => RunCommandStep {
            program: "greentic-flow".to_string(),
            args: build_args_with_fallback(
                vec!["new".to_string()],
                answers,
                &[("id", "--id"), ("out", "--out")],
                vec!["new".to_string(), "--help".to_string()],
            ),
            destructive: false,
        },
        ("flow", "wire") => RunCommandStep {
            program: "greentic-flow".to_string(),
            args: build_args_with_fallback(
                vec!["add-step".to_string()],
                answers,
                &[
                    ("flow", "--flow"),
                    ("coordinate", "--coordinate"),
                    ("after", "--after"),
                ],
                vec!["add-step".to_string(), "--help".to_string()],
            ),
            destructive: false,
        },
        ("bundle", "create") => RunCommandStep {
            program: "greentic-operator".to_string(),
            args: build_args_with_fallback(
                vec!["bundle".to_string(), "create".to_string()],
                answers,
                &[("name", "--name"), ("out", "--out")],
                vec![
                    "bundle".to_string(),
                    "create".to_string(),
                    "--help".to_string(),
                ],
            ),
            destructive: false,
        },
        ("dev", "doctor") => RunCommandStep {
            program: "greentic-flow".to_string(),
            args: build_dev_doctor_args(answers),
            destructive: false,
        },
        ("dev", "run") => RunCommandStep {
            program: "greentic-runner-cli".to_string(),
            args: build_args_with_fallback(
                vec![],
                answers,
                &[
                    ("pack", "--pack"),
                    ("entry", "--entry"),
                    ("input", "--input"),
                    ("json", "--json"),
                ],
                vec!["--help".to_string()],
            ),
            destructive: false,
        },
        _ => bail!("unsupported wizard mapping for `{target}.{mode}`"),
    };
    Ok(step)
}

fn build_args(
    mut base: Vec<String>,
    answers: &WizardAnswers,
    mapping: &[(&str, &str)],
) -> Vec<String> {
    for (key, flag) in mapping {
        if *flag == "--json" {
            if bool_from_answers(answers, key).unwrap_or(false) {
                base.push(flag.to_string());
            }
            continue;
        }
        if let Some(value) = string_from_answers(answers, key) {
            base.push(flag.to_string());
            base.push(value);
        }
    }
    base
}

fn build_args_with_fallback(
    base: Vec<String>,
    answers: &WizardAnswers,
    mapping: &[(&str, &str)],
    fallback: Vec<String>,
) -> Vec<String> {
    let args = build_args(base, answers, mapping);
    if args == fallback[..fallback.len().saturating_sub(1)] {
        return fallback;
    }
    args
}

fn build_dev_doctor_args(answers: &WizardAnswers) -> Vec<String> {
    let mut args = vec!["doctor".to_string()];
    if let Some(flow) = string_from_answers(answers, "flow") {
        args.push(flow);
    }
    if bool_from_answers(answers, "json").unwrap_or(false) {
        args.push("--json".to_string());
    }
    if args.len() == 1 {
        args.push("--help".to_string());
    }
    args
}

fn string_from_answers(answers: &WizardAnswers, key: &str) -> Option<String> {
    answers
        .data
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn bool_from_answers(answers: &WizardAnswers, key: &str) -> Option<bool> {
    answers.data.get(key).and_then(|v| v.as_bool())
}

fn extract_provider_refs(answers: &WizardAnswers) -> Vec<(String, String)> {
    let Some(obj) = answers
        .data
        .get("provider_refs")
        .and_then(|v| v.as_object())
    else {
        return Vec::new();
    };
    let mut refs = obj
        .iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
        .collect::<Vec<_>>();
    refs.sort_by(|a, b| a.0.cmp(&b.0));
    refs
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ProviderRequest, ShellWizardProvider, WizardProvider};
    use crate::wizard::plan::{WizardAnswers, WizardFrontend, WizardStep};

    #[test]
    fn build_plan_uses_answer_flags_for_pack_build() {
        let provider = ShellWizardProvider;
        let plan = provider
            .build_plan(&ProviderRequest {
                target: "pack".to_string(),
                mode: "build".to_string(),
                frontend: WizardFrontend::Json,
                locale: "en-US".to_string(),
                answers: WizardAnswers {
                    data: json!({
                        "in": ".",
                        "gtpack_out": "dist/out.gtpack",
                    }),
                },
            })
            .expect("build plan");

        let cmd = match plan.steps.last().expect("run command step") {
            WizardStep::RunCommand(cmd) => cmd,
            other => panic!("expected RunCommand step, got {other:?}"),
        };
        assert_eq!(cmd.program, "greentic-pack");
        assert_eq!(
            cmd.args,
            vec!["build", "--in", ".", "--gtpack-out", "dist/out.gtpack"]
        );
    }

    #[test]
    fn build_plan_records_provider_refs_in_inputs() {
        let provider = ShellWizardProvider;
        let plan = provider
            .build_plan(&ProviderRequest {
                target: "flow".to_string(),
                mode: "create".to_string(),
                frontend: WizardFrontend::Json,
                locale: "en-US".to_string(),
                answers: WizardAnswers {
                    data: json!({
                        "provider_refs": {
                            "pack": "pack://demo@sha256:abc",
                            "component": "component://demo@1.0.0"
                        }
                    }),
                },
            })
            .expect("build plan");

        assert_eq!(
            plan.inputs.get("provider_refs.component"),
            Some(&"component://demo@1.0.0".to_string())
        );
        assert_eq!(
            plan.inputs.get("provider_refs.pack"),
            Some(&"pack://demo@sha256:abc".to_string())
        );
    }

    #[test]
    fn build_plan_dev_doctor_uses_positional_flow() {
        let provider = ShellWizardProvider;
        let plan = provider
            .build_plan(&ProviderRequest {
                target: "dev".to_string(),
                mode: "doctor".to_string(),
                frontend: WizardFrontend::Json,
                locale: "en-US".to_string(),
                answers: WizardAnswers {
                    data: json!({"flow":"flows/main.ygtc", "json": true}),
                },
            })
            .expect("build plan");

        let cmd = match plan.steps.last().expect("run command step") {
            WizardStep::RunCommand(cmd) => cmd,
            other => panic!("expected RunCommand step, got {other:?}"),
        };
        assert_eq!(cmd.args, vec!["doctor", "flows/main.ygtc", "--json"]);
    }

    #[test]
    fn build_plan_dev_run_without_answers_falls_back_to_help() {
        let provider = ShellWizardProvider;
        let plan = provider
            .build_plan(&ProviderRequest {
                target: "dev".to_string(),
                mode: "run".to_string(),
                frontend: WizardFrontend::Json,
                locale: "en-US".to_string(),
                answers: WizardAnswers { data: json!({}) },
            })
            .expect("build plan");

        let cmd = match plan.steps.last().expect("run command step") {
            WizardStep::RunCommand(cmd) => cmd,
            other => panic!("expected RunCommand step, got {other:?}"),
        };
        assert_eq!(cmd.args, vec!["--help"]);
    }
}
