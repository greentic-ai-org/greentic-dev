//! Integration tests for `greentic-dev dw` store-API client commands.
//!
//! Mirrors the httpmock precedent in `tests/distributor_dev.rs`: spin up a
//! local mock store server, drive the library entry points, and assert on the
//! HTTP contract and on-disk results.

use std::fs;

use anyhow::Result;
use greentic_dev::dw_cmd::{DwCommand, DwInstallArgs, DwPublishArgs, run};
use httpmock::MockServer;
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

#[test]
fn install_verifies_sha_and_writes_file() -> Result<()> {
    if std::net::TcpListener::bind("127.0.0.1:0").is_err() {
        eprintln!("Skipping test; cannot bind local port in this environment");
        return Ok(());
    }
    let server = MockServer::start();
    let artifact = b"fake-gtpack-bytes".to_vec();
    let sha = sha256_hex(&artifact);

    let meta = server.mock(|when, then| {
        when.method("GET")
            .path("/api/v1/agentic-workers/acme.greeter/0.1.0");
        then.status(200)
            .header("content-type", "application/json")
            .json_body(json!({
                "describe": {"kind": "AgenticWorker"},
                "manifestSha256": "ab".repeat(32),
                "artifactSha256": sha,
                "secretsPolicyPresent": true,
                "publishedAt": "2026-01-01T00:00:00Z",
            }));
    });
    let art = server.mock(|when, then| {
        when.method("GET")
            .path("/api/v1/agentic-workers/acme.greeter/0.1.0/artifact");
        then.status(200).body(artifact.clone());
    });

    let out = tempdir().unwrap();
    let args = DwInstallArgs {
        name: "acme.greeter".into(),
        version: "0.1.0".into(),
        store: server.base_url(),
        out: Some(out.path().to_path_buf()),
    };
    run(DwCommand::Install(args), "en")?;

    meta.assert();
    art.assert();
    let dest = out.path().join("acme.greeter-0.1.0.gtpack");
    assert!(dest.exists(), "artifact should be written");
    assert_eq!(fs::read(&dest).unwrap(), artifact);
    Ok(())
}

#[test]
fn install_rejects_sha_mismatch_without_writing() -> Result<()> {
    if std::net::TcpListener::bind("127.0.0.1:0").is_err() {
        eprintln!("Skipping test; cannot bind local port in this environment");
        return Ok(());
    }
    let server = MockServer::start();
    let artifact = b"actual-bytes".to_vec();
    let wrong_sha = sha256_hex(b"different-bytes");

    server.mock(|when, then| {
        when.method("GET")
            .path("/api/v1/agentic-workers/acme.greeter/0.1.0");
        then.status(200).json_body(json!({
            "describe": {},
            "manifestSha256": "ab".repeat(32),
            "artifactSha256": wrong_sha,
            "secretsPolicyPresent": false,
            "publishedAt": "2026-01-01T00:00:00Z",
        }));
    });
    server.mock(|when, then| {
        when.method("GET")
            .path("/api/v1/agentic-workers/acme.greeter/0.1.0/artifact");
        then.status(200).body(artifact);
    });

    let out = tempdir().unwrap();
    let args = DwInstallArgs {
        name: "acme.greeter".into(),
        version: "0.1.0".into(),
        store: server.base_url(),
        out: Some(out.path().to_path_buf()),
    };
    let err = run(DwCommand::Install(args), "en").unwrap_err();
    assert!(
        err.to_string().contains("mismatch"),
        "expected sha mismatch error, got: {err}"
    );
    let dest = out.path().join("acme.greeter-0.1.0.gtpack");
    assert!(!dest.exists(), "no file must be written on sha mismatch");
    Ok(())
}

#[test]
fn install_errors_when_metadata_missing_sha() -> Result<()> {
    if std::net::TcpListener::bind("127.0.0.1:0").is_err() {
        eprintln!("Skipping test; cannot bind local port in this environment");
        return Ok(());
    }
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method("GET")
            .path("/api/v1/agentic-workers/acme.greeter/0.1.0");
        then.status(200)
            .json_body(json!({"describe": {}, "manifestSha256": "x"}));
    });

    let out = tempdir().unwrap();
    let args = DwInstallArgs {
        name: "acme.greeter".into(),
        version: "0.1.0".into(),
        store: server.base_url(),
        out: Some(out.path().to_path_buf()),
    };
    let err = run(DwCommand::Install(args), "en").unwrap_err();
    assert!(err.to_string().contains("artifactSha256"), "got: {err}");
    Ok(())
}

#[test]
fn publish_sends_metadata_envelope_and_bearer_token() -> Result<()> {
    if std::net::TcpListener::bind("127.0.0.1:0").is_err() {
        eprintln!("Skipping test; cannot bind local port in this environment");
        return Ok(());
    }
    let server = MockServer::start();
    let workspace = tempdir().unwrap();

    let describe = json!({
        "apiVersion": "v2",
        "kind": "AgenticWorker",
        "manifestSha256": "ab".repeat(32),
        "metadata": {"id": "acme.greeter", "version": "0.1.0"},
    });
    let describe_path = workspace.path().join("describe.json");
    fs::write(&describe_path, serde_json::to_vec(&describe).unwrap()).unwrap();

    let artifact = b"published-gtpack".to_vec();
    let artifact_path = workspace.path().join("worker.gtpack");
    fs::write(&artifact_path, &artifact).unwrap();
    let expected_sha = sha256_hex(&artifact);

    // Match on the multipart body carrying the envelope sha + bearer token.
    let publish = server.mock(|when, then| {
        when.method("POST")
            .path("/api/v1/agentic-workers")
            .header("authorization", "Bearer test-token")
            .body_includes(expected_sha.clone())
            .body_includes("\"describe\"")
            .body_includes("\"artifactSha256\"");
        then.status(201).json_body(json!({
            "name": "acme.greeter",
            "version": "0.1.0",
            "artifactSha256": expected_sha,
            "manifestSha256": "ab".repeat(32),
            "signature": "sig",
            "secretsPolicyPresent": false,
        }));
    });

    let args = DwPublishArgs {
        artifact: artifact_path,
        describe: describe_path,
        store: server.base_url(),
        token: Some("test-token".into()),
    };
    run(DwCommand::Publish(args), "en")?;
    publish.assert();
    Ok(())
}
