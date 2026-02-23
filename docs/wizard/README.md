# greentic-dev wizard

`greentic-dev wizard` is a deterministic orchestration entrypoint for developer workbench workflows.

## Commands

- `greentic-dev wizard run --target <...> --mode <...> [--dry-run|--execute]`
- `greentic-dev wizard replay --answers <path> [--dry-run|--execute]`

Frontend behavior:

- `--frontend text`: human-readable plan summary
- `--frontend json`: canonical deterministic plan JSON
- `--frontend adaptive-card`: Adaptive Card JSON payload containing the plan

## Supported PR-01 Target Modes

- `operator.create`
- `pack.create`
- `pack.build`
- `component.scaffold`
- `component.build`
- `flow.create`
- `flow.wire`
- `bundle.create`
- `dev.doctor`
- `dev.run`

## PR-01 Status

Implemented in this PR:

- Deterministic plan-first orchestration (`plan_version: 1`)
- `wizard run` / `wizard replay`
- Dry-run default and `--dry-run` / `--execute` mutual exclusion
- Interactive confirmation + non-interactive gating
- Shell-provider registry for PR-01 target/mode set
- High-level step modeling with `RunCommand` fallback
- Persistence of `answers.json`, `plan.json`, `exec.log`
- Replay support
- Command allowlist, unsafe arg blocking, destructive-step gating
- Execute-time program version capture + replay pin validation
- Frontends: `text`, `json`, `adaptive-card` (JSON payload)
- Audit doc and snapshot-based deterministic tests

Follow-up (outside PR-01):

- Deeper multi-step orchestration per target (beyond phase-1 shell mapping)
- Rich provider-native QaSpec UI flows across repos
- Transport integration for adaptive cards (Teams/WebChat/etc.)
- Broader digest pinning from remote resolvers

## Execution Rules

- Default mode is dry-run when neither `--dry-run` nor `--execute` is set.
- `--dry-run` and `--execute` are mutually exclusive.
- If both are provided, the CLI errors with: `Choose one of --dry-run or --execute.`
- Execute requires explicit consent:
  - Interactive TTY: prompt `Execute plan? [y/N]` unless `--yes`.
  - Non-interactive: require `--yes` or `--non-interactive`.

## Persistence

- Default output: `.greentic/wizard/<run-id>/`
- `--out` overrides the full output directory.
- Persisted files:
  - `answers.json`
  - `plan.json`
  - `exec.log` (when executed)

## Safety

- `RunCommand` steps execute as `{program,args}` (no shell string).
- Default allowlist covers:
  - `greentic-pack`
  - `greentic-component`
  - `greentic-flow`
  - `greentic-operator`
  - `greentic-runner-cli`
- Commands outside allowlist require `--unsafe-commands`.
- Destructive operations require `--allow-destructive` when present in plan steps.
