# PR-02: Add `greentic-dev codeql` Command

Repo: `greentic-dev`

## Summary

Add a top-level `greentic-dev codeql` command that reads GitHub Code Scanning alerts for the current repository and branch, filters them to CodeQL findings, and emits a concise Markdown prompt or JSON report suitable for a coding agent.

The command should inspect existing GitHub-hosted alert data only. It must not run CodeQL locally, upload SARIF, parse GitHub Actions logs, or modify source code.

## Current Codebase Fit

- CLI definitions live in `src/cli.rs` as a top-level `Command` enum plus argument structs.
- Runtime routing lives in `src/main.rs`.
- Non-passthrough command logic lives in dedicated modules such as `src/coverage_cmd.rs`, `src/release_cmd.rs`, and `src/mcp_cmd.rs`, exported from `src/lib.rs`.
- Localized help keys are stored under `i18n/*.json`; adding a command should include the new command and argument help keys so `cli_i18n_audit` continues to pass.
- CLI parsing tests live in `tests/cli_release.rs` and similar integration-style parser tests.

Implement this as a new `src/codeql_cmd.rs` module, not as passthrough behavior.

## CLI

Add:

```bash
greentic-dev codeql
greentic-dev codeql --format markdown
greentic-dev codeql --format json
greentic-dev codeql --prompt
greentic-dev codeql --no-errors
greentic-dev codeql --no-errors --severity error
greentic-dev codeql --no-errors --severity error,warning
greentic-dev codeql --severity error,warning,note
greentic-dev codeql --security-severity critical,high,medium,low
greentic-dev codeql --state open
greentic-dev codeql --state open,dismissed,fixed
greentic-dev codeql --repo OWNER/REPO
greentic-dev codeql --branch BRANCH
```

### Arguments

- `--format <markdown|json>`
  - Default: `markdown`.
  - `markdown` prints a human-readable report.
  - `json` prints machine-readable normalized alert data.
- `--prompt`
  - Convenience mode for a coding-agent-ready Markdown prompt.
  - Equivalent to Markdown output with stronger instruction text and no extra operational chatter.
- `--no-errors`
  - Policy-check mode for CI/scripts.
  - Query CodeQL alerts the same way as the normal command, but force the state filter to open alerts only.
  - If any matching alerts remain after severity/security-severity filtering, exit with policy-violation code `2`.
  - If no matching alerts remain, exit `0`.
  - Still print a short summary unless `--format json` is selected.
  - When `--no-errors` is set and `--severity` is not supplied, default `--severity` to `error`.
  - This default avoids blocking development on lower-priority warnings while still allowing stricter gates such as `--severity error,warning`.
- `--severity <LIST>`
  - Comma-separated filter against Code Scanning `rule.severity` values such as `error`, `warning`, and `note`.
  - Default: no severity filtering beyond state/tool filters, except `--no-errors` defaults this to `error`.
- `--security-severity <LIST>`
  - Comma-separated filter against CodeQL security severity levels when present, such as `critical`, `high`, `medium`, and `low`.
  - This is separate from `--severity` because GitHub Code Scanning exposes quality severity and security severity differently.
- `--state <LIST>`
  - Comma-separated states.
  - Default: `open`.
  - Supported values should match GitHub Code Scanning states: `open`, `dismissed`, `fixed`.
- `--repo OWNER/REPO`
  - Optional explicit GitHub repository.
  - When absent, detect from `git remote get-url origin`.
- `--branch BRANCH`
  - Optional explicit branch.
  - When absent, detect from `git branch --show-current`.

## Behavior

### Repository Detection

Resolve repository in this order:

1. Use `--repo OWNER/REPO` if supplied.
2. Otherwise run `git remote get-url origin`.
3. Parse common GitHub remote forms:
   - `https://github.com/OWNER/REPO.git`
   - `git@github.com:OWNER/REPO.git`
   - `ssh://git@github.com/OWNER/REPO.git`

Reject non-GitHub remotes with a clear error explaining that `--repo OWNER/REPO` can be used.

### Branch And Commit Detection

Resolve branch in this order:

1. Use `--branch BRANCH` if supplied.
2. Otherwise run `git branch --show-current`.

Resolve commit with:

```bash
git rev-parse HEAD
```

If branch detection returns an empty string, fail with a clear detached-HEAD message unless `--branch` was supplied.

### GitHub Query

Use the GitHub CLI by default:

```bash
gh api "/repos/{owner}/{repo}/code-scanning/alerts?state={state}&ref=refs/heads/{branch}"
```

For multiple states, make one request per state and merge the responses. Keep the implementation testable by separating command execution from response parsing.

Filter to CodeQL alerts only. Treat tool names case-insensitively and accept `CodeQL` from either:

- `tool.name`
- `most_recent_instance.tool.name`

Use GitHub Code Scanning response fields:

- `number`
- `state`
- `html_url`
- `rule.id`
- `rule.name`
- `rule.description`
- `rule.severity`
- `rule.security_severity_level`
- `most_recent_instance.message.text`
- `most_recent_instance.location.path`
- `most_recent_instance.location.start_line`
- `most_recent_instance.location.start_column`
- `most_recent_instance.location.end_line`
- `most_recent_instance.location.end_column`

Normalize missing optional fields to `null` in JSON and to `(unknown)` or omitted values in Markdown.

## Output

### Markdown Report

Default Markdown output should be pasteable into a coding agent:

```markdown
# CodeQL Issues

Repository: greenticai/greentic
Branch: feature/foo
Commit: abc123

Found 3 open CodeQL alerts.

## Coding-agent instructions

Fix the following CodeQL findings. Complete as much as possible without repeatedly asking for permission. Routine code changes, tests, and formatting are pre-authorised. Avoid destructive changes.

### 1. error - rust/path-injection

File: crates/foo/src/bar.rs:44

Rule: `rust/path-injection`
Description: User-controlled data flows into a filesystem path.
Security severity: high

Message:
User-controlled data flows into a filesystem path.

Suggested fix:
Validate or constrain the path before use. Prefer canonicalization plus allowlisted base directory checks.

Alert:
https://github.com/OWNER/REPO/security/code-scanning/1
```

### JSON Report

JSON output should be stable and machine-readable:

```json
{
  "repository": "greenticai/greentic",
  "branch": "feature/foo",
  "commit": "abc123",
  "state_filter": ["open"],
  "severity_filter": ["error", "warning"],
  "security_severity_filter": ["high"],
  "alerts": [
    {
      "number": 1,
      "state": "open",
      "tool": "CodeQL",
      "severity": "error",
      "security_severity": "high",
      "rule_id": "rust/path-injection",
      "rule_name": "Path injection",
      "rule_description": "User-controlled data flows into a filesystem path.",
      "message": "User-controlled data flows into a filesystem path.",
      "path": "crates/foo/src/bar.rs",
      "start_line": 44,
      "start_column": 12,
      "end_line": 44,
      "end_column": 31,
      "html_url": "https://github.com/OWNER/REPO/security/code-scanning/1"
    }
  ],
  "summary": {
    "count": 1,
    "by_severity": {
      "error": 1
    },
    "by_security_severity": {
      "high": 1
    }
  }
}
```

### Empty Results

If no matching CodeQL alerts are found:

- Exit successfully.
- Markdown should print the repository, branch, commit, filters, and `Found 0 CodeQL alerts.`
- JSON should print an empty `alerts` array and zero counts.

### Policy Check Mode

`--no-errors` is designed for scripts and CI gates:

```bash
greentic-dev codeql --no-errors
greentic-dev codeql --no-errors --severity error
greentic-dev codeql --no-errors --severity error,warning
```

Behavior:

- Query CodeQL alerts the same as the normal command.
- Consider open alerts only.
- Apply selected severity and security-severity filters.
- Exit `2` if any matching alerts remain.
- Exit `0` if no matching alerts remain.
- Exit `3` for operational errors such as missing `gh`, authentication failure, API failure, invalid repo, or detached HEAD.

Exit codes:

| Code | Meaning |
| --- | --- |
| 0 | No matching CodeQL alerts |
| 2 | Matching CodeQL alerts found; policy violation |
| 3 | Operational error, such as API/auth/git/CLI failure |

Do not use generic exit code `1` for the policy path. Scripts must be able to distinguish "your code has CodeQL findings" from "the tool failed."

## Error Handling

- Missing `gh`:
  - Exit `3`.
  - Print: `GitHub CLI (gh) is required. Install it and run gh auth login.`
- `gh` authentication failure or HTTP 401/403:
  - Exit `3`.
  - Print a clear `gh auth login` message.
- Code Scanning disabled or unavailable:
  - Exit successfully if GitHub returns an empty list.
  - Exit `3` with a clear message if GitHub returns an operational API error.
- Missing git remote, non-GitHub remote, detached HEAD without `--branch`, or invalid `OWNER/REPO`:
  - Exit `3` with actionable guidance.

## Implementation Notes

- Add `Command::Codeql(CodeqlArgs)` to `src/cli.rs`.
- Add `CodeqlArgs` and small value enums or string parsers for:
  - output format
  - state list
  - severity list
  - security severity list
  - `--no-errors` policy mode
- Add `("codeql", "cli.command.codeql.about")` to localized help setup.
- Add argument help keys:
  - `cli.command.codeql.about`
  - `cli.command.codeql.format`
  - `cli.command.codeql.prompt`
  - `cli.command.codeql.no_errors`
  - `cli.command.codeql.severity`
  - `cli.command.codeql.security_severity`
  - `cli.command.codeql.state`
  - `cli.command.codeql.repo`
  - `cli.command.codeql.branch`
- Add the new keys to every `i18n/*.json` file, using English fallback text where translations are not available yet.
- Add `pub mod codeql_cmd;` to `src/lib.rs`.
- Route `Command::Codeql(args)` in `src/main.rs` to `codeql_cmd::run(args)`.
- Keep the command implementation independent from GitHub connector/plugin APIs; this is a CLI feature that should work in normal developer shells through `gh`.
- Do not add new Greentic shared types for this; the report structs are command-local DTOs.

## Tests

Add focused unit tests in `src/codeql_cmd.rs` for:

- parsing `OWNER/REPO`
- parsing GitHub HTTPS remotes
- parsing GitHub SSH remotes
- rejecting non-GitHub remotes
- parsing comma-separated state/severity filters
- `--no-errors` defaults severity to `error` when `--severity` is absent
- `--no-errors` preserves explicit severities when supplied
- `--no-errors` forces open-state policy checks
- parsing Code Scanning API responses
- filtering to CodeQL tool names only
- filtering by state
- filtering by `rule.severity`
- filtering by `rule.security_severity_level`
- empty alert list rendering
- Markdown report rendering
- prompt rendering
- JSON report rendering
- missing `gh` / unauthenticated error classification if the command-runner abstraction supports it
- policy exit code `2` when matching alerts exist
- success exit code `0` when no matching alerts exist
- operational exit code `3` for command/API/auth/git failures

Add CLI parsing tests for:

```bash
greentic-dev codeql
greentic-dev codeql --format json
greentic-dev codeql --prompt
greentic-dev codeql --no-errors
greentic-dev codeql --no-errors --severity error
greentic-dev codeql --no-errors --severity error,warning
greentic-dev codeql --severity error,warning
greentic-dev codeql --security-severity high,critical
greentic-dev codeql --state open,dismissed
greentic-dev codeql --repo greenticai/greentic-dev --branch feature/foo
```

Update the i18n audit/help tests as needed.

## Acceptance Criteria

- `greentic-dev codeql --format markdown` prints a clean Markdown report.
- `greentic-dev codeql --prompt` prints a coding-agent-ready prompt.
- `greentic-dev codeql --format json` prints stable machine-readable JSON.
- `greentic-dev codeql --no-errors` exits `0` when no open error-level CodeQL alerts are found.
- `greentic-dev codeql --no-errors` exits `2` when open error-level CodeQL alerts are found.
- `greentic-dev codeql --no-errors --severity error,warning` exits `2` when open error or warning CodeQL alerts are found.
- Operational failures exit `3`.
- Normal report mode exits `0` when no matching alerts are found.
- Normal report mode exits non-zero only for real operational failures.
- `--no-errors` mode uses non-zero exit `2` for matching alerts and non-zero exit `3` for operational failures.
- The command does not run CodeQL locally, upload SARIF, parse Actions logs, or modify code.
- The implementation follows the existing `src/cli.rs` / `src/main.rs` / command-module structure.
- `cargo test` coverage includes command parsing, response parsing, filtering, and rendering.
- `./ci/local_check.sh` passes.
