use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};

use crate::config::{DistributorProfileConfig, GreenticConfig};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DevIntent {
    Dev,
    Runtime,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DevArtifactKind {
    Component,
    Pack,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DevResolveRequest {
    pub coordinate: String,
    pub intent: DevIntent,
    pub platform: Option<String>,
    pub features: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DevLicenseType {
    Free,
    Commercial,
    Trial,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DevLicenseInfo {
    pub license_type: DevLicenseType,
    pub id: Option<String>,
    pub requires_acceptance: bool,
    pub checkout_url: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DevResolveResponse {
    pub kind: DevArtifactKind,
    pub name: String,
    pub version: String,
    pub coordinate: String,
    pub artifact_id: String,
    pub artifact_download_path: String,
    pub digest: Option<String>,
    pub license: DevLicenseInfo,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DevLicenseRequiredErrorBody {
    pub error: String,
    pub coordinate: String,
    pub message: String,
    pub checkout_url: String,
}

#[derive(Debug)]
pub enum DevDistributorError {
    Http(reqwest::Error),
    LicenseRequired(DevLicenseRequiredErrorBody),
    Status(reqwest::StatusCode, Option<String>),
    InvalidResponse(anyhow::Error),
}

impl std::fmt::Display for DevDistributorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DevDistributorError::Http(err) => write!(f, "http error: {err}"),
            DevDistributorError::LicenseRequired(body) => {
                write!(f, "{} (checkout: {})", body.message, body.checkout_url)
            }
            DevDistributorError::Status(code, body) => {
                if let Some(body) = body {
                    write!(f, "unexpected status {code}: {body}")
                } else {
                    write!(f, "unexpected status {code}")
                }
            }
            DevDistributorError::InvalidResponse(err) => write!(f, "invalid response: {err}"),
        }
    }
}

impl std::error::Error for DevDistributorError {}

impl From<reqwest::Error> for DevDistributorError {
    fn from(value: reqwest::Error) -> Self {
        DevDistributorError::Http(value)
    }
}

#[derive(Debug, Clone)]
pub struct DistributorProfile {
    pub name: String,
    pub url: String,
    pub token: Option<String>,
}

impl DistributorProfile {
    fn from_pair(name: &str, cfg: &DistributorProfileConfig) -> Result<Self> {
        let token = resolve_token(cfg.token.clone())?;
        let url = cfg.url.trim_end_matches('/').to_string();
        Ok(Self {
            name: name.to_string(),
            url,
            token,
        })
    }
}

pub fn resolve_profile(
    config: &GreenticConfig,
    profile_arg: Option<&str>,
) -> Result<DistributorProfile> {
    let env_profile = std::env::var("GREENTIC_DISTRIBUTOR_PROFILE").ok();
    let profile_name = profile_arg.or(env_profile.as_deref()).unwrap_or("default");
    let map: &HashMap<String, DistributorProfileConfig> = &config.distributor.profiles;
    let Some(profile_cfg) = map.get(profile_name) else {
        bail!(
            "distributor profile `{profile_name}` not found; configure it in ~/.greentic/config.toml"
        );
    };
    DistributorProfile::from_pair(profile_name, profile_cfg)
}

fn resolve_token(raw: Option<String>) -> Result<Option<String>> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    if let Some(rest) = raw.strip_prefix("env:") {
        let value = std::env::var(rest)
            .with_context(|| format!("failed to resolve env var {rest} for distributor token"))?;
        Ok(Some(value))
    } else {
        Ok(Some(raw))
    }
}

#[derive(Debug, Clone)]
pub struct DevDistributorClient {
    base_url: String,
    auth_token: Option<String>,
    http: Client,
}

impl DevDistributorClient {
    pub fn from_profile(profile: DistributorProfile) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self {
            base_url: profile.url,
            auth_token: profile.token,
            http: client,
        })
    }

    pub fn resolve(
        &self,
        req: &DevResolveRequest,
    ) -> Result<DevResolveResponse, DevDistributorError> {
        let url = format!("{}/v1/resolve", self.base_url);
        let mut builder = self.http.post(url).header(CONTENT_TYPE, "application/json");
        if let Some(token) = &self.auth_token {
            builder = builder.header(AUTHORIZATION, format!("Bearer {token}"));
        }
        let response = builder.json(req).send()?;
        if response.status().as_u16() == 402 {
            let body: DevLicenseRequiredErrorBody = response
                .json()
                .map_err(|err| DevDistributorError::InvalidResponse(err.into()))?;
            return Err(DevDistributorError::LicenseRequired(body));
        }
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().ok();
            return Err(DevDistributorError::Status(status, body));
        }
        response
            .json::<DevResolveResponse>()
            .map_err(|err| DevDistributorError::InvalidResponse(err.into()))
    }

    pub fn download_artifact(
        &self,
        download_path: &str,
    ) -> Result<bytes::Bytes, DevDistributorError> {
        let trimmed_base = self.base_url.trim_end_matches('/');
        let trimmed_path = download_path.trim_start_matches('/');
        let url = format!("{trimmed_base}/{trimmed_path}");
        let mut builder = self.http.get(url);
        if let Some(token) = &self.auth_token {
            builder = builder.header(AUTHORIZATION, format!("Bearer {token}"));
        }
        let response = builder.send()?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().ok();
            return Err(DevDistributorError::Status(status, body));
        }
        response.bytes().map_err(DevDistributorError::Http)
    }
}
