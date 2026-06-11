//! Thin store-API client for agentic worker publish/install.
//!
//! Mirrors the HTTP distribution precedent in [`crate::distributor`] and the
//! designer's `StoreClient` (greentic-designer `src/store/client.rs`). No
//! upstream CLI exists for these operations, so the commands live natively in
//! greentic-dev rather than passing through to a delegated binary.
//!
//! The store endpoints are:
//! - `POST {store}/api/v1/agentic-workers` — multipart publish (auth required).
//! - `GET  {store}/api/v1/agentic-workers/{name}/{version}` — version metadata.
//! - `GET  {store}/api/v1/agentic-workers/{name}/{version}/artifact` — bytes.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use reqwest::blocking::{Client, multipart};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::i18n;

/// Environment variable consulted for the publish bearer token when `--token`
/// is not supplied.
const STORE_TOKEN_ENV: &str = "GREENTIC_STORE_TOKEN";

#[derive(Subcommand, Debug)]
pub enum DwCommand {
    /// cli.command.dw.publish.about
    Publish(DwPublishArgs),
    /// cli.command.dw.install.about
    Install(DwInstallArgs),
}

#[derive(Args, Debug, Clone)]
pub struct DwPublishArgs {
    /// cli.command.dw.publish.artifact
    pub artifact: PathBuf,
    /// cli.command.dw.publish.describe
    #[arg(long = "describe")]
    pub describe: PathBuf,
    /// cli.command.dw.publish.store
    #[arg(long = "store")]
    pub store: String,
    /// cli.command.dw.publish.token
    #[arg(long = "token")]
    pub token: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct DwInstallArgs {
    /// cli.command.dw.install.name
    pub name: String,
    /// cli.command.dw.install.version
    pub version: String,
    /// cli.command.dw.install.store
    #[arg(long = "store")]
    pub store: String,
    /// cli.command.dw.install.out
    #[arg(long = "out")]
    pub out: Option<PathBuf>,
}

pub fn run(cmd: DwCommand, locale: &str) -> Result<()> {
    match cmd {
        DwCommand::Publish(args) => run_publish(&args, locale),
        DwCommand::Install(args) => run_install(&args, locale),
    }
}

/// Build a blocking HTTP client matching the distributor's configuration.
fn http_client() -> Result<Client> {
    Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .context("failed to build HTTP client")
}

/// Percent-encode a single URL path segment. Keeps unreserved characters
/// (RFC 3986: ALPHA / DIGIT / `-._~`) and encodes everything else, so worker
/// names like `acme.greeter` and semver versions pass through unchanged while
/// any unexpected character is escaped safely.
fn encode_path_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Lowercase hex sha256 of the given bytes (matches the store's encoding).
fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

/// Resolve the publish bearer token: `--token` flag, then `GREENTIC_STORE_TOKEN`.
///
/// Mirrors the distributor precedence style (explicit value wins over env).
fn resolve_publish_token(flag: Option<&str>, locale: &str) -> Result<String> {
    if let Some(token) = flag {
        let trimmed = token.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    if let Ok(value) = std::env::var(STORE_TOKEN_ENV) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    bail!(
        "{}",
        i18n::tf(
            locale,
            "runtime.dw.error.missing_token",
            &[("env", STORE_TOKEN_ENV.to_string())],
        )
    )
}

/// Construct the `{describe, artifactSha256}` metadata envelope the store
/// publish endpoint expects (see greentic-store-server publish handler /
/// `PublishRequest`). The describe document is passed through verbatim.
fn build_publish_metadata(describe: Value, artifact_sha256: &str) -> Value {
    serde_json::json!({
        "describe": describe,
        "artifactSha256": artifact_sha256,
    })
}

fn run_publish(args: &DwPublishArgs, locale: &str) -> Result<()> {
    let token = resolve_publish_token(args.token.as_deref(), locale)?;
    let base = args.store.trim_end_matches('/').to_string();

    let describe_raw = std::fs::read(&args.describe).with_context(|| {
        i18n::tf(
            locale,
            "runtime.dw.error.read_describe",
            &[("path", args.describe.display().to_string())],
        )
    })?;
    let describe: Value = serde_json::from_slice(&describe_raw).with_context(|| {
        i18n::tf(
            locale,
            "runtime.dw.error.parse_describe",
            &[("path", args.describe.display().to_string())],
        )
    })?;

    let artifact_bytes = std::fs::read(&args.artifact).with_context(|| {
        i18n::tf(
            locale,
            "runtime.dw.error.read_artifact",
            &[("path", args.artifact.display().to_string())],
        )
    })?;
    let artifact_sha256 = sha256_hex(&artifact_bytes);
    let metadata = build_publish_metadata(describe, &artifact_sha256);
    let metadata_str =
        serde_json::to_string(&metadata).context("failed to serialize publish metadata")?;

    let artifact_part = multipart::Part::bytes(artifact_bytes)
        .file_name("artifact.gtpack")
        .mime_str("application/octet-stream")
        .context("failed to build artifact multipart part")?;
    let form = multipart::Form::new()
        .text("metadata", metadata_str)
        .part("artifact", artifact_part);

    let url = format!("{base}/api/v1/agentic-workers");
    let client = http_client()?;
    let response = client
        .post(&url)
        .bearer_auth(&token)
        .multipart(form)
        .send()
        .with_context(|| {
            i18n::tf(
                locale,
                "runtime.dw.error.request_failed",
                &[("url", url.clone())],
            )
        })?;

    let status = response.status();
    let body = response.text().unwrap_or_default();
    if !status.is_success() {
        eprintln!(
            "{}",
            i18n::tf(
                locale,
                "runtime.dw.error.publish_status",
                &[("status", status.to_string()), ("body", body)],
            )
        );
        std::process::exit(1);
    }

    // Pretty-print the store's JSON response; fall back to raw text.
    match serde_json::from_str::<Value>(&body) {
        Ok(json) => {
            let pretty = serde_json::to_string_pretty(&json).unwrap_or_else(|_| body.clone());
            println!("{pretty}");
        }
        Err(_) => println!("{body}"),
    }
    Ok(())
}

fn run_install(args: &DwInstallArgs, locale: &str) -> Result<()> {
    let base = args.store.trim_end_matches('/').to_string();
    let client = http_client()?;

    // 1. Fetch version metadata to learn the expected artifact sha256.
    let meta_url = format!(
        "{base}/api/v1/agentic-workers/{name}/{version}",
        name = encode_path_segment(&args.name),
        version = encode_path_segment(&args.version),
    );
    let meta_resp = client.get(&meta_url).send().with_context(|| {
        i18n::tf(
            locale,
            "runtime.dw.error.request_failed",
            &[("url", meta_url.clone())],
        )
    })?;
    let meta_status = meta_resp.status();
    let meta_body = meta_resp.text().unwrap_or_default();
    if !meta_status.is_success() {
        bail!(
            "{}",
            i18n::tf(
                locale,
                "runtime.dw.error.metadata_status",
                &[("status", meta_status.to_string()), ("body", meta_body)],
            )
        );
    }
    let metadata: Value =
        serde_json::from_str(&meta_body).context("store metadata response was not valid JSON")?;
    let expected_sha = extract_artifact_sha256(&metadata)
        .ok_or_else(|| anyhow::anyhow!(i18n::t(locale, "runtime.dw.error.missing_sha")))?;

    // 2. Download the artifact bytes.
    let artifact_url = format!(
        "{base}/api/v1/agentic-workers/{name}/{version}/artifact",
        name = encode_path_segment(&args.name),
        version = encode_path_segment(&args.version),
    );
    let artifact_resp = client.get(&artifact_url).send().with_context(|| {
        i18n::tf(
            locale,
            "runtime.dw.error.request_failed",
            &[("url", artifact_url.clone())],
        )
    })?;
    let artifact_status = artifact_resp.status();
    if !artifact_status.is_success() {
        let body = artifact_resp.text().unwrap_or_default();
        bail!(
            "{}",
            i18n::tf(
                locale,
                "runtime.dw.error.artifact_status",
                &[("status", artifact_status.to_string()), ("body", body)],
            )
        );
    }
    let artifact_bytes = artifact_resp
        .bytes()
        .context("failed to read artifact bytes from store")?;

    // 3. Verify sha256 — hard fail with NO file written on mismatch.
    let actual_sha = sha256_hex(&artifact_bytes);
    if !actual_sha.eq_ignore_ascii_case(&expected_sha) {
        bail!(
            "{}",
            i18n::tf(
                locale,
                "runtime.dw.error.sha_mismatch",
                &[("expected", expected_sha), ("actual", actual_sha)],
            )
        );
    }

    // 4. Write atomically (temp + rename) into the output directory.
    let out_dir = args.out.clone().unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&out_dir).with_context(|| {
        i18n::tf(
            locale,
            "runtime.dw.error.create_out_dir",
            &[("path", out_dir.display().to_string())],
        )
    })?;
    let file_name = format!("{}-{}.gtpack", args.name, args.version);
    let dest = out_dir.join(&file_name);
    write_atomic(&out_dir, &dest, &artifact_bytes).with_context(|| {
        i18n::tf(
            locale,
            "runtime.dw.error.write_artifact",
            &[("path", dest.display().to_string())],
        )
    })?;

    println!(
        "{}",
        i18n::tf(
            locale,
            "runtime.dw.install.success",
            &[("path", dest.display().to_string()), ("sha", actual_sha),],
        )
    );
    Ok(())
}

/// Pull `artifactSha256` out of the version metadata JSON (camelCase per the
/// store's `AgenticWorkerVersionMetadata` serialization).
fn extract_artifact_sha256(metadata: &Value) -> Option<String> {
    metadata
        .get("artifactSha256")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Write `bytes` to `dest` via a temporary file in `dir` then rename, so a
/// failed/interrupted write never leaves a partial `.gtpack` behind.
fn write_atomic(dir: &Path, dest: &Path, bytes: &[u8]) -> Result<()> {
    let mut tmp = tempfile::Builder::new()
        .prefix(".dw-install-")
        .suffix(".gtpack.tmp")
        .tempfile_in(dir)
        .context("failed to create temp file")?;
    use std::io::Write;
    tmp.write_all(bytes).context("failed to write temp file")?;
    tmp.flush().context("failed to flush temp file")?;
    tmp.persist(dest)
        .map_err(|e| e.error)
        .context("failed to move temp file into place")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sha256_hex_matches_known_vector() {
        // sha256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn build_publish_metadata_wraps_describe_and_sha() {
        let describe = json!({
            "apiVersion": "v2",
            "kind": "AgenticWorker",
            "manifestSha256": "ab".repeat(32),
            "metadata": {"id": "acme.greeter", "version": "0.1.0"},
        });
        let sha = "cd".repeat(32);
        let envelope = build_publish_metadata(describe.clone(), &sha);
        assert_eq!(envelope["describe"], describe);
        assert_eq!(envelope["artifactSha256"], json!(sha));
        // Exactly the two fields the store's PublishRequest deserializes.
        let obj = envelope.as_object().unwrap();
        assert_eq!(obj.len(), 2);
        assert!(obj.contains_key("describe"));
        assert!(obj.contains_key("artifactSha256"));
    }

    #[test]
    fn resolve_token_prefers_flag() {
        let token = resolve_publish_token(Some("flag-token"), "en").unwrap();
        assert_eq!(token, "flag-token");
    }

    #[test]
    fn resolve_token_errors_when_absent() {
        // Ensure env is unset for this assertion.
        // Safe: single-threaded test, no other reader of this var here.
        unsafe {
            std::env::remove_var(STORE_TOKEN_ENV);
        }
        let err = resolve_publish_token(None, "en").unwrap_err();
        assert!(err.to_string().contains(STORE_TOKEN_ENV));
    }

    #[test]
    fn extract_artifact_sha256_reads_camel_case() {
        let meta = json!({"artifactSha256": "deadbeef", "manifestSha256": "x"});
        assert_eq!(extract_artifact_sha256(&meta).as_deref(), Some("deadbeef"));
    }

    #[test]
    fn extract_artifact_sha256_absent_is_none() {
        let meta = json!({"manifestSha256": "x"});
        assert!(extract_artifact_sha256(&meta).is_none());
    }
}
