//! Pull answer documents from an OCI registry for `--answers oci://…` / `ghcr://…`.
//!
//! The greentic-demo publish pipeline pushes each answer document to GHCR as a
//! raw-JSON OCI artifact
//! (`oras push --artifact-type application/vnd.greentic.answers.<kind>.v1+json`).
//! This module resolves such a reference to the document's JSON text so the
//! launcher can treat it exactly like a local file or an `https://` URL.

use anyhow::{Context, Result, bail};
use greentic_distributor_client::{OciPackFetcher, PackFetchOptions};

/// Layer media type for a `create` answer document.
const ANSWERS_CREATE_MEDIA_TYPE: &str = "application/vnd.greentic.answers.create.v1+json";
/// Layer media type for a `setup` answer document.
const ANSWERS_SETUP_MEDIA_TYPE: &str = "application/vnd.greentic.answers.setup.v1+json";

/// GHCR namespace the `ghcr://` shortcut expands into.
const GHCR_DEFAULT_NAMESPACE: &str = "ghcr.io/greenticai";

/// True when `raw` names a remote OCI reference rather than a path or http(s) URL.
pub fn is_oci_reference(raw: &str) -> bool {
    raw.starts_with("oci://") || raw.starts_with("ghcr://")
}

/// Pull the answer document referenced by `raw` and return its JSON text.
pub fn fetch_oci_answers_text(raw: &str) -> Result<String> {
    let oci_reference = map_answers_reference(raw)?;
    let answers_media_types = vec![
        ANSWERS_CREATE_MEDIA_TYPE.to_string(),
        ANSWERS_SETUP_MEDIA_TYPE.to_string(),
    ];
    let fetcher: OciPackFetcher = OciPackFetcher::new(PackFetchOptions {
        allow_tags: true,
        offline: false,
        accepted_layer_media_types: answers_media_types.clone(),
        preferred_layer_media_types: answers_media_types,
        cache_dir: std::env::temp_dir()
            .join("greentic-dev")
            .join("answers-cache"),
        ..PackFetchOptions::default()
    });
    let runtime =
        tokio::runtime::Runtime::new().context("create tokio runtime for answers pull")?;
    let bytes = runtime
        .block_on(fetcher.fetch_pack(&oci_reference))
        .with_context(|| format!("failed to pull answers document from {raw}"))?;
    String::from_utf8(bytes).with_context(|| format!("answers document {raw} is not valid UTF-8"))
}

/// Map a scheme-prefixed reference to a bare OCI reference.
///
/// * `oci://X` -> `X` (verbatim).
/// * `ghcr://path[:tag|@sha256:...]` -> `ghcr.io/greenticai/path[:tag|@sha256:...]`,
///   defaulting to `:latest` when neither a tag nor a digest is present.
fn map_answers_reference(raw: &str) -> Result<String> {
    if let Some(rest) = raw.strip_prefix("oci://") {
        if rest.is_empty() {
            bail!("answers reference {raw} is missing an OCI path");
        }
        return Ok(rest.to_string());
    }
    if let Some(rest) = raw.strip_prefix("ghcr://") {
        let trimmed = rest.trim_start_matches('/');
        if trimmed.is_empty() {
            bail!("answers reference {raw} is missing a GHCR path");
        }
        return Ok(ensure_tag_or_digest(&format!(
            "{GHCR_DEFAULT_NAMESPACE}/{trimmed}"
        )));
    }
    bail!("answers reference {raw} is not a supported remote reference; use oci:// or ghcr://")
}

/// Append `:latest` when a reference carries neither an explicit tag nor a digest.
fn ensure_tag_or_digest(reference: &str) -> String {
    if reference.contains("@sha256:") {
        return reference.to_string();
    }
    let last_segment = reference.rsplit('/').next().unwrap_or(reference);
    if last_segment.contains(':') {
        return reference.to_string();
    }
    format!("{reference}:latest")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_oci_and_ghcr_references() {
        assert!(is_oci_reference(
            "oci://ghcr.io/greenticai/answers/x/create:latest"
        ));
        assert!(is_oci_reference("ghcr://answers/x/create"));
        assert!(!is_oci_reference("https://example.com/x.json"));
        assert!(!is_oci_reference("demos/x-create-answers.json"));
    }

    #[test]
    fn oci_reference_passes_through_verbatim() {
        assert_eq!(
            map_answers_reference("oci://ghcr.io/greenticai/answers/quickstart/create:latest")
                .unwrap(),
            "ghcr.io/greenticai/answers/quickstart/create:latest"
        );
    }

    #[test]
    fn ghcr_shortcut_expands_namespace_and_defaults_latest() {
        assert_eq!(
            map_answers_reference("ghcr://answers/quickstart/create").unwrap(),
            "ghcr.io/greenticai/answers/quickstart/create:latest"
        );
    }

    #[test]
    fn ghcr_shortcut_preserves_explicit_tag_and_digest() {
        assert_eq!(
            map_answers_reference("ghcr://answers/quickstart/create:1.2.3").unwrap(),
            "ghcr.io/greenticai/answers/quickstart/create:1.2.3"
        );
        assert_eq!(
            map_answers_reference("ghcr://answers/quickstart/create@sha256:abc").unwrap(),
            "ghcr.io/greenticai/answers/quickstart/create@sha256:abc"
        );
    }

    #[test]
    fn rejects_unsupported_and_empty_references() {
        assert!(map_answers_reference("https://example.com/x.json").is_err());
        assert!(map_answers_reference("oci://").is_err());
        assert!(map_answers_reference("ghcr://").is_err());
    }
}
