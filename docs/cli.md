# greentic-dev CLI Guide

`greentic-dev` is a passthrough wrapper over upstream CLIs, plus a launcher wizard.

## Flow (passthrough to greentic-flow)

- `flow ...` delegates directly to `greentic-flow` (including `--help`).

## Component (passthrough to greentic-component)

- `component ...` delegates directly to `greentic-component` (including `--help`).

## Pack (passthrough to greentic-pack; `pack run` uses greentic-runner-cli)

- `pack ...` delegates to `greentic-pack`.
- `pack run ...` delegates to `greentic-runner-cli`.

## GUI / Secrets / MCP

- `gui ...` delegates to `greentic-gui`.
- `secrets ...` wraps `greentic-secrets` convenience flows.
- `mcp doctor` is available when the optional feature is enabled.

## CBOR

- `cbor <file>.cbor` decodes a CBOR payload and prints pretty JSON.

## Wizard (Launcher-Only)

- `greentic-dev wizard`
- `greentic-dev wizard --dry-run`
- `greentic-dev wizard validate --answers <FILE>`
- `greentic-dev wizard apply --answers <FILE>`

Behavior:

- `wizard` is interactive and prompts for launcher action:
  - pack path -> delegates to `greentic-pack wizard`
  - bundle path -> delegates to `greentic-operator wizard`
- `--dry-run` builds/renders plan without delegated execution.
- `validate` builds plan from `AnswerDocument` without delegated execution.
- `apply` builds and executes delegation from `AnswerDocument`.
- `wizard run` and `wizard replay` are removed.

AnswerDocument identity is strict:

- `wizard_id`: `greentic-dev.wizard.launcher.main`
- `schema_id`: `greentic-dev.launcher.main`

Non-launcher IDs are rejected by `validate` / `apply`.

## Tips

- Missing delegated tools are not auto-installed. Use `greentic-dev install tools` (or `--latest`).
- Environment overrides:
  - `GREENTIC_DEV_BIN_GREENTIC_FLOW`
  - `GREENTIC_DEV_BIN_GREENTIC_COMPONENT`
  - `GREENTIC_DEV_BIN_GREENTIC_PACK`
  - `GREENTIC_DEV_BIN_GREENTIC_RUNNER_CLI`
  - `GREENTIC_DEV_BIN_GREENTIC_GUI`
  - `GREENTIC_DEV_BIN_GREENTIC_SECRETS`
