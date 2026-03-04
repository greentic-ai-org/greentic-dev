use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum WizardFrontend {
    Text,
    Json,
    AdaptiveCard,
}

impl WizardFrontend {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "text" => Some(Self::Text),
            "json" => Some(Self::Json),
            "adaptive-card" => Some(Self::AdaptiveCard),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WizardPlan {
    pub plan_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    pub metadata: WizardPlanMetadata,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, String>,
    pub steps: Vec<WizardStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WizardPlanMetadata {
    pub target: String,
    pub mode: String,
    pub locale: String,
    pub frontend: WizardFrontend,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum WizardStep {
    LaunchPackWizard,
    LaunchBundleWizard,
    RunCommand(RunCommandStep),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunCommandStep {
    pub program: String,
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub destructive: bool,
}

fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WizardAnswers {
    pub data: serde_json::Value,
}

impl Default for WizardAnswers {
    fn default() -> Self {
        Self {
            data: serde_json::Value::Object(Default::default()),
        }
    }
}
