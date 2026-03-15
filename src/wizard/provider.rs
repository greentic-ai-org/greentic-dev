use anyhow::{Result, bail};
use std::path::PathBuf;

use crate::wizard::plan::{
    RunCommandStep, WizardAnswers, WizardFrontend, WizardPlan, WizardPlanMetadata, WizardStep,
};

pub trait WizardProvider {
    fn build_plan(&self, req: &ProviderRequest) -> Result<WizardPlan>;
}

#[derive(Debug, Clone)]
pub struct ProviderRequest {
    pub frontend: WizardFrontend,
    pub locale: String,
    pub dry_run: bool,
    pub answers: WizardAnswers,
    pub delegated_answers_path: Option<PathBuf>,
}

pub struct ShellWizardProvider;

impl WizardProvider for ShellWizardProvider {
    fn build_plan(&self, req: &ProviderRequest) -> Result<WizardPlan> {
        let selected_action = selected_action(&req.answers)?;
        let args = delegated_command_args(req);

        let (program, semantic_step) = match selected_action {
            "pack" => ("greentic-pack".to_string(), WizardStep::LaunchPackWizard),
            "bundle" => (
                "greentic-bundle".to_string(),
                WizardStep::LaunchBundleWizard,
            ),
            other => bail!("unsupported selected_action `{other}`; expected `pack` or `bundle`"),
        };

        Ok(WizardPlan {
            plan_version: 1,
            created_at: None,
            metadata: WizardPlanMetadata {
                target: "launcher".to_string(),
                mode: "main".to_string(),
                locale: req.locale.clone(),
                frontend: req.frontend.clone(),
            },
            inputs: Default::default(),
            steps: vec![
                semantic_step,
                WizardStep::RunCommand(RunCommandStep {
                    program,
                    args,
                    destructive: false,
                }),
            ],
        })
    }
}

fn selected_action(answers: &WizardAnswers) -> Result<&str> {
    let Some(action) = answers.data.get("selected_action").and_then(|v| v.as_str()) else {
        bail!("missing required answers.selected_action (`pack` or `bundle`)");
    };
    Ok(action)
}

fn delegated_command_args(req: &ProviderRequest) -> Vec<String> {
    let mut args = vec!["wizard".to_string()];
    if let Some(path) = &req.delegated_answers_path {
        args.extend([
            "apply".to_string(),
            "--answers".to_string(),
            path.display().to_string(),
        ]);
    }
    if req.dry_run {
        args.push("--dry-run".to_string());
    }
    args
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ProviderRequest, ShellWizardProvider, WizardProvider};
    use crate::wizard::plan::{WizardAnswers, WizardFrontend, WizardStep};

    #[test]
    fn build_plan_pack_apply() {
        let provider = ShellWizardProvider;
        let plan = provider
            .build_plan(&ProviderRequest {
                frontend: WizardFrontend::Json,
                locale: "en-US".to_string(),
                dry_run: false,
                answers: WizardAnswers {
                    data: json!({"selected_action":"pack"}),
                },
                delegated_answers_path: None,
            })
            .expect("build plan");

        assert_eq!(plan.metadata.target, "launcher");
        assert_eq!(plan.metadata.mode, "main");
        let cmd = match plan.steps.last().expect("run step") {
            WizardStep::RunCommand(cmd) => cmd,
            other => panic!("expected RunCommand step, got {other:?}"),
        };
        assert_eq!(cmd.program, "greentic-pack");
        assert_eq!(cmd.args, vec!["wizard"]);
    }

    #[test]
    fn build_plan_bundle_dry_run() {
        let provider = ShellWizardProvider;
        let plan = provider
            .build_plan(&ProviderRequest {
                frontend: WizardFrontend::Json,
                locale: "en-US".to_string(),
                dry_run: true,
                answers: WizardAnswers {
                    data: json!({"selected_action":"bundle"}),
                },
                delegated_answers_path: None,
            })
            .expect("build plan");

        let cmd = match plan.steps.last().expect("run step") {
            WizardStep::RunCommand(cmd) => cmd,
            other => panic!("expected RunCommand step, got {other:?}"),
        };
        assert_eq!(cmd.program, "greentic-bundle");
        assert_eq!(cmd.args, vec!["wizard", "--dry-run"]);
    }

    #[test]
    fn build_plan_uses_delegated_answers_when_present() {
        let provider = ShellWizardProvider;
        let plan = provider
            .build_plan(&ProviderRequest {
                frontend: WizardFrontend::Json,
                locale: "en-US".to_string(),
                dry_run: false,
                answers: WizardAnswers {
                    data: json!({"selected_action":"bundle"}),
                },
                delegated_answers_path: Some("/tmp/delegated-answers.json".into()),
            })
            .expect("build plan");

        let cmd = match plan.steps.last().expect("run step") {
            WizardStep::RunCommand(cmd) => cmd,
            other => panic!("expected RunCommand step, got {other:?}"),
        };
        assert_eq!(cmd.program, "greentic-bundle");
        assert_eq!(
            cmd.args,
            vec![
                "wizard",
                "apply",
                "--answers",
                "/tmp/delegated-answers.json",
            ]
        );
    }
}
