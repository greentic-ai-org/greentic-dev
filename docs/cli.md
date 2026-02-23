# greentic-dev CLI Guide

`greentic-dev` is a passthrough wrapper over the upstream CLIs. Flags and behavior come from:
- [`greentic-component/docs/cli.md`](../greentic-component/docs/cli.md)
- [`greentic-flow/docs/cli.md`](../greentic-flow/docs/cli.md)
- [`greentic-pack/docs/cli.md`](../greentic-pack/docs/cli.md)

Below is a quick map of what’s available and how to use it from this repo. For authoritative flag lists, follow the upstream links.

## Flow (passthrough to greentic-flow)
- `flow ...` delegates directly to `greentic-flow` (including `--help`).

Reference: [`greentic-flow/docs/cli.md`](../greentic-flow/docs/cli.md)

## Component (passthrough to greentic-component)
- `component ...` delegates directly to `greentic-component` (including `--help`).

Reference: [`greentic-component/docs/cli.md`](../greentic-component/docs/cli.md)

## Pack (passthrough to greentic-pack; `pack run` uses greentic-runner-cli)
- `pack ...` delegates to `greentic-pack`.
- `pack run ...` delegates to `greentic-runner-cli` (including `--help`).

Reference: [`greentic-pack/docs/cli.md`](../greentic-pack/docs/cli.md)

## GUI / Secrets / MCP
- `gui ...` delegates directly to `greentic-gui` (including `--help`).
- `secrets …` wraps `greentic-secrets`.
- `mcp doctor` is available when the optional feature is enabled.

## CBOR
- `cbor <file>.cbor` decodes a CBOR payload and prints pretty JSON.

## Wizard
- `wizard run --target <...> --mode <...>` builds a deterministic plan (`plan_version: 1`) and prints JSON.
- Default behavior is dry-run when neither `--dry-run` nor `--execute` is passed.
- `--dry-run` and `--execute` are mutually exclusive.
- `wizard replay --answers <path>` reuses persisted `answers.json` + sibling `plan.json`.
- See `docs/wizard/README.md` for details.

## Tips
- Missing delegated tools are not auto-installed. Install them with `greentic-dev install tools` (or `greentic-dev install tools --latest`).
- Environment overrides: `GREENTIC_DEV_BIN_GREENTIC_FLOW`, `GREENTIC_DEV_BIN_GREENTIC_COMPONENT`, `GREENTIC_DEV_BIN_GREENTIC_PACK`, `GREENTIC_DEV_BIN_GREENTIC_RUNNER_CLI`, `GREENTIC_DEV_BIN_GREENTIC_GUI`, `GREENTIC_DEV_BIN_GREENTIC_SECRETS` to point at local builds.
- Prefer positional args where upstream uses them (e.g., `flow doctor <flow>`); the wrapper does not add extra semantics.
