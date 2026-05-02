use clap::Parser;
use greentic_dev::cli::{Cli, Command, SecurityFormat};

#[test]
fn parses_security_defaults() {
    let cli = Cli::try_parse_from(["greentic-dev", "security"]).unwrap();
    let Command::Security(args) = cli.command else {
        panic!("expected security command");
    };
    assert_eq!(args.format, SecurityFormat::Markdown);
    assert_eq!(args.state, "open");
    assert!(!args.prompt);
    assert!(!args.no_errors);
}

#[test]
fn parses_security_json_format() {
    let cli = Cli::try_parse_from(["greentic-dev", "security", "--format", "json"]).unwrap();
    let Command::Security(args) = cli.command else {
        panic!("expected security command");
    };
    assert_eq!(args.format, SecurityFormat::Json);
}

#[test]
fn parses_security_prompt() {
    let cli = Cli::try_parse_from(["greentic-dev", "security", "--prompt"]).unwrap();
    let Command::Security(args) = cli.command else {
        panic!("expected security command");
    };
    assert!(args.prompt);
}

#[test]
fn parses_security_no_errors() {
    let cli = Cli::try_parse_from(["greentic-dev", "security", "--no-errors"]).unwrap();
    let Command::Security(args) = cli.command else {
        panic!("expected security command");
    };
    assert!(args.no_errors);
}

#[test]
fn parses_security_ignore_errors() {
    let cli = Cli::try_parse_from(["greentic-dev", "security", "--ignore-errors"]).unwrap();
    let Command::Security(args) = cli.command else {
        panic!("expected security command");
    };
    assert!(args.no_errors);
}

#[test]
fn parses_security_no_errors_with_error_severity() {
    let cli = Cli::try_parse_from([
        "greentic-dev",
        "security",
        "--no-errors",
        "--severity",
        "error",
    ])
    .unwrap();
    let Command::Security(args) = cli.command else {
        panic!("expected security command");
    };
    assert!(args.no_errors);
    assert_eq!(args.severity.as_deref(), Some("error"));
}

#[test]
fn parses_security_no_errors_with_multiple_severities() {
    let cli = Cli::try_parse_from([
        "greentic-dev",
        "security",
        "--no-errors",
        "--severity",
        "error,warning",
    ])
    .unwrap();
    let Command::Security(args) = cli.command else {
        panic!("expected security command");
    };
    assert_eq!(args.severity.as_deref(), Some("error,warning"));
}

#[test]
fn parses_security_severity_filter() {
    let cli =
        Cli::try_parse_from(["greentic-dev", "security", "--severity", "error,warning"]).unwrap();
    let Command::Security(args) = cli.command else {
        panic!("expected security command");
    };
    assert_eq!(args.severity.as_deref(), Some("error,warning"));
}

#[test]
fn parses_security_security_severity_filter() {
    let cli = Cli::try_parse_from([
        "greentic-dev",
        "security",
        "--security-severity",
        "high,critical",
    ])
    .unwrap();
    let Command::Security(args) = cli.command else {
        panic!("expected security command");
    };
    assert_eq!(args.security_severity.as_deref(), Some("high,critical"));
}

#[test]
fn parses_security_state_filter() {
    let cli =
        Cli::try_parse_from(["greentic-dev", "security", "--state", "open,dismissed"]).unwrap();
    let Command::Security(args) = cli.command else {
        panic!("expected security command");
    };
    assert_eq!(args.state, "open,dismissed");
}

#[test]
fn parses_security_repo_and_branch() {
    let cli = Cli::try_parse_from([
        "greentic-dev",
        "security",
        "--repo",
        "greenticai/greentic-dev",
        "--branch",
        "feature/foo",
    ])
    .unwrap();
    let Command::Security(args) = cli.command else {
        panic!("expected security command");
    };
    assert_eq!(args.repo.as_deref(), Some("greenticai/greentic-dev"));
    assert_eq!(args.branch.as_deref(), Some("feature/foo"));
}
