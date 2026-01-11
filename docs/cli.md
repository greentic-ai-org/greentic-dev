# greentic-dev CLI Guide

`greentic-dev` is a passthrough wrapper over the upstream CLIs. Flags and behavior come from:
- [`greentic-component/docs/cli.md`](../greentic-component/docs/cli.md)
- [`greentic-flow/docs/cli.md`](../greentic-flow/docs/cli.md)
- [`greentic-pack/docs/cli.md`](../greentic-pack/docs/cli.md)

Below is a quick map of what’s available and how to use it from this repo. For authoritative flag lists, follow the upstream links.

## Flow (delegates to greentic-flow)
- `flow doctor <flow.ygtc> [--json]`: Validate flows against component schemas. Equivalent to `greentic-flow doctor`.
- `flow add-step --flow <path> --after <node> --mode <default|config> [...]`: Passthrough to `greentic-flow add-step`; supports routing shorthands (`--routing-out`, `--routing-reply`, `--routing-next`, `--routing-multi-to`, `--routing-json`, `--routing-to-anchor`), component refs (`--component`, `--component-ref`, `--local-wasm`, `--pin`), and config-flow inputs (`--config-flow`, `--answers`, `--answers-file`).

Reference: [`greentic-flow/docs/cli.md`](../greentic-flow/docs/cli.md)

## Component (delegates to greentic-component)
- Scaffold: `component new --name demo --path ./components/demo --non-interactive`
- Build: `component build --manifest components/demo/component.manifest.json`
- Doctor: `component doctor <wasm> --manifest <manifest.json>`
- Inspect/describe/pack/templates: all flags match upstream.

Reference: [`greentic-component/docs/cli.md`](../greentic-component/docs/cli.md)

## Pack (delegates to greentic-pack; `pack run` uses greentic-runner)
- Authoring helpers: `pack components`, `pack update`, `pack config`, `pack gui`, `pack doctor`/`pack inspect`.
- Build: `pack build -- --in . --gtpack-out dist/app.gtpack`
- Doctor/Inspect: `pack doctor <pack.gtpack>` (or `inspect` on older versions)
- Run: `pack run --pack dist/app.gtpack [--offline] [--mocks on] [--artifacts dist/artifacts]` (passthrough to greentic-runner)

Reference: [`greentic-pack/docs/cli.md`](../greentic-pack/docs/cli.md)

## GUI / Secrets / MCP
- `gui serve`, `gui pack-dev` delegate to `greentic-gui`.
- `secrets …` wraps `greentic-secrets`.
- `mcp doctor` is available when the optional feature is enabled.

## Tips
- Add `--verbose` to see which binary was invoked.
- Environment overrides: `GREENTIC_DEV_BIN_GREENTIC_FLOW`, `GREENTIC_DEV_BIN_GREENTIC_COMPONENT`, `GREENTIC_DEV_BIN_GREENTIC_PACK`, `GREENTIC_DEV_BIN_GREENTIC_RUNNER` to point at local builds.
- Prefer positional args where upstream uses them (e.g., `flow doctor <flow>`)—the wrapper does not add extra semantics.
