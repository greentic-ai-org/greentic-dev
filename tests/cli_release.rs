use clap::Parser;
use greentic_dev::cli::{Cli, Command, ReleaseCommand};

#[test]
fn parses_release_generate() {
    let cli = Cli::try_parse_from([
        "greentic-dev",
        "release",
        "generate",
        "--release",
        "1.0.5",
        "--from",
        "latest",
        "--token",
        "env:GHCR_TOKEN",
    ])
    .unwrap();
    let Command::Release(ReleaseCommand::Generate(args)) = cli.command else {
        panic!("expected release generate");
    };
    assert_eq!(args.release, "1.0.5");
    assert_eq!(args.from, "latest");
    assert_eq!(args.token.as_deref(), Some("env:GHCR_TOKEN"));
}

#[test]
fn parses_release_publish_with_tag_and_force() {
    let cli = Cli::try_parse_from([
        "greentic-dev",
        "release",
        "publish",
        "--release",
        "1.0.5",
        "--from",
        "latest",
        "--tag",
        "rc",
        "--force",
    ])
    .unwrap();
    let Command::Release(ReleaseCommand::Publish(args)) = cli.command else {
        panic!("expected release publish");
    };
    assert_eq!(args.release.as_deref(), Some("1.0.5"));
    assert_eq!(args.from.as_deref(), Some("latest"));
    assert_eq!(args.tag.as_deref(), Some("rc"));
    assert!(args.force);
}

#[test]
fn parses_release_publish_from_manifest() {
    let cli = Cli::try_parse_from([
        "greentic-dev",
        "release",
        "publish",
        "--manifest",
        "dist/toolchains/gtc-1.0.12.json",
        "--tag",
        "stable",
        "--token",
        "env:GHCR_TOKEN",
        "--force",
    ])
    .unwrap();
    let Command::Release(ReleaseCommand::Publish(args)) = cli.command else {
        panic!("expected release publish");
    };
    assert_eq!(args.release, None);
    assert_eq!(
        args.manifest.as_deref(),
        Some(std::path::Path::new("dist/toolchains/gtc-1.0.12.json"))
    );
    assert_eq!(args.tag.as_deref(), Some("stable"));
    assert_eq!(args.token.as_deref(), Some("env:GHCR_TOKEN"));
    assert!(args.force);
}

#[test]
fn parses_release_publish_from_manifest_with_explicit_release() {
    let cli = Cli::try_parse_from([
        "greentic-dev",
        "release",
        "publish",
        "--manifest",
        "dist/toolchains/gtc-1.0.13.json",
        "--release",
        "1.0.13",
        "--token",
        "env:GHCR_TOKEN",
        "--force",
    ])
    .unwrap();
    let Command::Release(ReleaseCommand::Publish(args)) = cli.command else {
        panic!("expected release publish");
    };
    assert_eq!(args.release.as_deref(), Some("1.0.13"));
    assert_eq!(
        args.manifest.as_deref(),
        Some(std::path::Path::new("dist/toolchains/gtc-1.0.13.json"))
    );
}

#[test]
fn parses_release_promote() {
    let cli = Cli::try_parse_from([
        "greentic-dev",
        "release",
        "promote",
        "--release",
        "1.0.5",
        "--tag",
        "stable",
    ])
    .unwrap();
    let Command::Release(ReleaseCommand::Promote(args)) = cli.command else {
        panic!("expected release promote");
    };
    assert_eq!(args.release, "1.0.5");
    assert_eq!(args.tag, "stable");
}

#[test]
fn parses_release_view_by_tag() {
    let cli = Cli::try_parse_from([
        "greentic-dev",
        "release",
        "view",
        "--tag",
        "stable",
        "--token",
        "env:GHCR_TOKEN",
    ])
    .unwrap();
    let Command::Release(ReleaseCommand::View(args)) = cli.command else {
        panic!("expected release view");
    };
    assert_eq!(args.release, None);
    assert_eq!(args.tag.as_deref(), Some("stable"));
    assert_eq!(args.token.as_deref(), Some("env:GHCR_TOKEN"));
}

#[test]
fn release_view_requires_release_or_tag() {
    let err = Cli::try_parse_from(["greentic-dev", "release", "view"]).unwrap_err();
    assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
}

#[test]
fn parses_release_latest() {
    let cli = Cli::try_parse_from([
        "greentic-dev",
        "release",
        "latest",
        "--token",
        "env:GHCR_TOKEN",
        "--force",
    ])
    .unwrap();
    let Command::Release(ReleaseCommand::Latest(args)) = cli.command else {
        panic!("expected release latest");
    };
    assert_eq!(args.token.as_deref(), Some("env:GHCR_TOKEN"));
    assert!(args.force);
}
