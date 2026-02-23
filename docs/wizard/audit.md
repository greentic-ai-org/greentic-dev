# Wizard Audit (PR-DEV-01)

- Existing CLI pattern:
  - `src/cli.rs` uses `clap` enums with top-level `Command` and nested subcommands.
  - `src/main.rs` dispatches subcommands directly and exits with passthrough command status.
  - New wizard command should follow this style as a top-level subcommand to avoid collisions.

- Existing QA integration points:
  - No dedicated wizard orchestration module currently exists in `greentic-dev`.
  - Repo contains flow/component fixtures and runtime helpers, but no shared QaSpec orchestration layer yet.
  - `greentic-dev` already delegates to canonical CLIs (`greentic-flow`, `greentic-component`, `greentic-pack`, etc.).

- Existing shell-out precedent:
  - `src/passthrough.rs` resolves binaries and executes with `std::process::Command`.
  - `src/secrets_cli.rs` and top-level command handlers use passthrough execution heavily.
  - Command delegation is an established pattern and is compatible with a provider trait + shell bridge.

- Recommended phase-1 integration mode:
  - Use shell-out providers behind a trait as the default implementation for PR-01.
  - Keep high-level plan steps in `greentic-dev` and avoid implementing pack/component/flow semantics locally.
  - Preserve deterministic, plan-first behavior with persisted plan/answers and replay.
