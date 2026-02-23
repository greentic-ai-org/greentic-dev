use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use crate::wizard::plan::{WizardAnswers, WizardPlan};

pub struct PersistedPaths {
    pub answers_path: PathBuf,
    pub plan_path: PathBuf,
    pub exec_log_path: PathBuf,
}

pub fn resolve_out_dir(out: Option<&Path>) -> PathBuf {
    if let Some(path) = out {
        return path.to_path_buf();
    }
    let run_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| format!("run-{}", d.as_secs()))
        .unwrap_or_else(|_| "run-unknown".to_string());
    PathBuf::from(".greentic").join("wizard").join(run_id)
}

pub fn prepare_dir(root: &Path) -> Result<PersistedPaths> {
    fs::create_dir_all(root).with_context(|| format!("failed to create {}", root.display()))?;
    Ok(PersistedPaths {
        answers_path: root.join("answers.json"),
        plan_path: root.join("plan.json"),
        exec_log_path: root.join("exec.log"),
    })
}

pub fn persist_plan_and_answers(
    paths: &PersistedPaths,
    answers: &WizardAnswers,
    plan: &WizardPlan,
) -> Result<()> {
    let answers_json =
        serde_json::to_string_pretty(&answers.data).context("render answers JSON")?;
    fs::write(&paths.answers_path, answers_json)
        .with_context(|| format!("failed to write {}", paths.answers_path.display()))?;

    let plan_json = serde_json::to_string_pretty(plan).context("render plan JSON")?;
    fs::write(&paths.plan_path, plan_json)
        .with_context(|| format!("failed to write {}", paths.plan_path.display()))?;
    Ok(())
}

pub fn load_replay(answers_path: &Path) -> Result<(WizardAnswers, WizardPlan, PathBuf)> {
    let parent = answers_path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "answers path must have a parent directory: {}",
            answers_path.display()
        )
    })?;
    let plan_path = parent.join("plan.json");
    if !plan_path.exists() {
        bail!(
            "replay requires {} next to {}",
            plan_path.display(),
            answers_path.display()
        );
    }

    let answers_raw = fs::read_to_string(answers_path)
        .with_context(|| format!("failed to read {}", answers_path.display()))?;
    let answers_val: serde_json::Value = serde_json::from_str(&answers_raw)
        .with_context(|| format!("failed to parse {}", answers_path.display()))?;
    let answers = WizardAnswers { data: answers_val };

    let plan_raw = fs::read_to_string(&plan_path)
        .with_context(|| format!("failed to read {}", plan_path.display()))?;
    let plan: WizardPlan = serde_json::from_str(&plan_raw)
        .with_context(|| format!("failed to parse {}", plan_path.display()))?;
    Ok((answers, plan, parent.to_path_buf()))
}
