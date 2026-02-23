# Dev Workbench Targets (PR-01 Scope)

Supported `target.mode` entries:

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

## Example Runs

- `pack.create`
  - `greentic-dev wizard run --target pack --mode create --answers answers-pack-create.json --out .greentic/wizard/pack-create`
  - Example answers keys: `dir`, `name`, `pack_id`

- `pack.build`
  - `greentic-dev wizard run --target pack --mode build --answers answers-pack-build.json`
  - Example answers keys: `in`, `gtpack_out`

- `component.scaffold`
  - `greentic-dev wizard run --target component --mode scaffold --answers answers-component-scaffold.json`
  - Example answers keys: `name`, `path`, `template`

- `component.build`
  - `greentic-dev wizard run --target component --mode build --answers answers-component-build.json`
  - Example answers keys: `manifest`

- `flow.create`
  - `greentic-dev wizard run --target flow --mode create --answers answers-flow-create.json`
  - Example answers keys: `id`, `out`

- `flow.wire`
  - `greentic-dev wizard run --target flow --mode wire --answers answers-flow-wire.json`
  - Example answers keys: `flow`, `coordinate`, `after`

- `bundle.create` / `operator.create`
  - `greentic-dev wizard run --target bundle --mode create --answers answers-bundle-create.json`
  - `greentic-dev wizard run --target operator --mode create --answers answers-operator-create.json`
  - Example answers keys: `name`, `out`

- `dev.doctor`
  - `greentic-dev wizard run --target dev --mode doctor --answers answers-dev-doctor.json`
  - Example answers keys: `flow`, `json`

- `dev.run`
  - `greentic-dev wizard run --target dev --mode run --answers answers-dev-run.json`
  - Example answers keys: `pack`, `entry`, `input`, `json`

## Integration Strategy

- Phase-1 uses shell-out providers behind a trait abstraction.
- Lower-level repos own apply/materialization logic.
- `greentic-dev` owns deterministic orchestration, safety policy, and replay plumbing.
- Current provider behavior: command argument construction is answer-key driven and intentionally minimal; richer flow chaining remains follow-up work.

## Frontends

- `text`
- `json`
- `adaptive-card` (JSON output only in PR-01)

## i18n

Locale is passed through and stored in plan metadata.

Fallback chain:

1. Provider locale strings
2. Provider default locale
3. Raw text
4. Key-as-text with warning
