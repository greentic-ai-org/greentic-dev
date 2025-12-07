# Greentic Flow

Generic schema, loader, and intermediate representation for YGTC flows composed of self-describing component nodes.

## Quickstart
```rust
use greentic_flow::{load_and_validate_bundle, resolve::resolve_parameters, loader, to_ir};

let yaml = std::fs::read_to_string("fixtures/weather_bot.ygtc")?;
let bundle = load_and_validate_bundle(&yaml, None)?;
println!("Bundle entry node: {}", bundle.entry);

let flow = loader::load_ygtc_from_str(&yaml, std::path::Path::new("schemas/ygtc.flow.schema.json"))?;
let ir = to_ir(flow)?;
let node = ir.nodes.get("forecast_weather").unwrap();
let resolved = resolve_parameters(&node.payload_expr, &ir.parameters, "nodes.forecast_weather")?;
# Ok::<_, greentic_flow::error::FlowError>(())
```

## Design Highlights
- JSON Schema (`schemas/ygtc.flow.schema.json`) enforces exactly one component key per node plus optional routing metadata.
- Loader converts YAML documents to `FlowDoc`, validates against the schema, extracts component metadata, and performs basic graph checks.
- IR (`FlowIR`) keeps nodes generic and serde-friendly so runtimes can post-process component payloads while exposing `NodeKind` classification for adapters.
- `NodeKind::Adapter` recognises node component strings shaped as `<namespace>.<adapter>.<operation>` and keeps the trailing segments joined so nested operations are preserved.
- `resolve::resolve_parameters` pre-resolves only `parameters.*` references, leaving other runtime bindings intact.
- `start` is optional; if omitted and an `in` node exists, the loader defaults `start` to `in`.

## Adapter Registry Format
Adapter-backed nodes can be linted against an on-disk catalog that maps `<namespace>.<adapter>` pairs to the operations they expose. The registry is JSON by default, with optional TOML support via the `toml` feature.

```json
{
  "adapters": {
    "messaging.telegram": ["sendMessage", "editMessage"],
    "email.google": ["send", "draft.create"]
  }
}
```

With the registry loaded, the `adapter_resolvable` rule reports any node whose component string cannot be found in the catalog.

## Development
- `cargo fmt --check`
- `cargo clippy -D warnings`
- `cargo test`

Fixtures under `fixtures/` mirror common success and failure scenarios.

Run all CI-equivalent checks locally via:

```
LOCAL_CHECK_ONLINE=1 ci/local_check.sh
```

Toggles:

- `LOCAL_CHECK_ONLINE=1` — enable networked checks (schema fetch, etc.)
- `LOCAL_CHECK_STRICT=1` — treat missing tools as immediate failures (default already fails required skips unless `LOCAL_CHECK_ALLOW_SKIP=1`)
- `LOCAL_CHECK_VERBOSE=1` — echo each command
- `LOCAL_CHECK_ALLOW_SKIP=1` — allow required CI steps to be skipped (not recommended)

## CLI

### Flow scaffolding

`greentic-flow` ships with a lightweight scaffolder for new `.ygtc` files:

```
cargo run --bin greentic-flow -- new flows/demo.ygtc --kind messaging
```

Flags of note:

- `--kind messaging|events|deployment` controls the template. `--kind deployment`
  is just sugar for `type: events` plus a first node that assumes access to the
  `greentic:deploy-plan@1.0.0` world.
- `--deployment` aliases `--kind deployment`.
- `--pack-manifest <path>` (or a local `manifest.yaml` if no flag is provided)
  lets the tool peek at pack metadata. It will:
  - default `--kind` to `deployment` when the pack declares `kind: deployment`,
  - append the new flow to the manifest's `flows:` array (path stored relative
    to the manifest directory), and
  - emit informational hints (for example, when a deployment pack scaffolds a
    messaging flow anyway).
- `--id`, `--description`, and `--force` cover the usual ergonomics.

Running the command writes a ready-to-edit `.ygtc` file and reports any hints.

### Flow linting

Run `cargo run --bin ygtc-lint -- <paths>` to validate flows. Example:

```
cargo run --bin ygtc-lint -- fixtures --schema schemas/ygtc.flow.schema.json
```

To enable adapter linting, provide `--registry`:

```
cargo run --bin ygtc-lint -- \
  --schema schemas/ygtc.flow.schema.json \
  --registry tests/data/registry_ok.json \
  tests/data/flow_ok.ygtc
```

For machine-readable CI, use `--json`; the command exits non-zero on any error and
prints the validated bundle plus diagnostics:

```
cargo run --quiet --bin ygtc-lint -- --json tests/data/flow_ok.ygtc
# { "ok": true, "bundle": { "id": "flow_ok", ... } }
```

Pipelines can also stream flows via stdin:

```
cat tests/data/flow_ok.ygtc | cargo run --quiet --bin ygtc-lint -- --json --stdin
```

And in CI you can assert the BLAKE3 hash is present:

```
ygtc-lint --json --stdin < flow.ygtc | jq -e '.ok and .hash_blake3 != null'
```

The CLI recursively walks any directories provided, only inspecting files with a `.ygtc` extension. Schema validation always runs; adapter checks are additive when a registry is supplied.

The shared flow schema is published from this repository at
`https://raw.githubusercontent.com/greentic-ai/greentic-flow/refs/heads/master/schemas/ygtc.flow.schema.json`
and matches the `$id` embedded in `schemas/ygtc.flow.schema.json`.

## Config flows (convention)

A config flow is a regular flow whose kind may be `component-config` (or any other string) and whose final node emits a payload shaped as:

```json
{ "node_id": "some_step", "node": { /* full node object with one component key plus routing */ } }
```

Tools like `greentic-dev` can execute these flows and splice the emitted node into another flow. The engine itself does not special-case config flows: node components such as `questions` (prompting for values) and `template` (rendering a JSON template) are handled like any other component.

For lightweight automation in this crate, `config_flow::run_config_flow` can execute simple config flows by seeding answers for `questions` fields and rendering the final `template` payload into `{ node_id, node }`.

## Deployment flows (events-based)

Deployment flows are standard `type: events` flows that operate on a
`DeploymentPlan` provided by hosting tooling. Use

```
greentic-flow new flows/deploy_stack.ygtc --kind deployment
```

to scaffold one quickly. The template creates a first node that highlights the
`greentic:deploy-plan@1.0.0` world so the component can read the plan and emit
status updates. Node IDs and component kinds remain opaque strings; nothing in
this crate hard-codes provider-specific behaviour.

See [`docs/deployment-flows.md`](docs/deployment-flows.md) for a deeper dive
covering plan access, CLI helpers, and authoring guidelines.

A pack may declare `kind: deployment` in its manifest to signal that most of its
flows are deployment-oriented. The scaffolder simply treats that as a hint and
emits an informational message if you add a messaging flow to such a pack. Mixed
packs remain perfectly valid.

## Environment
- `OTEL_EXPORTER_OTLP_ENDPOINT` (default `http://localhost:4317`) targets your collector.
- `RUST_LOG` controls log verbosity; e.g. `greentic_flow=info`.
- `OTEL_RESOURCE_ATTRIBUTES=deployment.environment=dev` tags spans with the active environment.

## Maintenance Notes
- Keep shared primitives flowing through `greentic-types` and `greentic-interfaces`.
- Prefer zero-copy patterns and stay within safe Rust (`#![forbid(unsafe_code)]` is enabled).
- Update the adapter registry fixtures under `tests/data/` when new adapters or operations are introduced.

## Releases & Publishing
- Crate versions are sourced directly from each crate's `Cargo.toml`.
- Every push to `master` compares the previous commit; if a crate version changed, a tag `<crate-name>-v<semver>` is created and pushed automatically.
- The publish workflow runs on the tagged commit and attempts to publish all changed crates to crates.io using `katyo/publish-crates@v2`.
- Publishing is idempotent: if the version already exists on crates.io, the workflow succeeds without error.
