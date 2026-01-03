# greentic-dev – schema-aware developer toolkit

`greentic-dev` is the command-line toolbox we use to design, validate, and iterate on Greentic components before they ever hit production. It bundles a schema-aware flow runner, mock services, a transcript viewer, and component tooling into one workspace so that building new automation feels repeatable and safe.

If you want to:

* prove that a flow YAML matches the latest component schema,
* spin up a new component repo that already understands describe APIs, JSON Schema, and CI guardrails,
* emulate Greentic services locally without real credentials, and
* inspect transcripts that show **which values came from defaults vs overrides**,

…then this repository is where you start.

---

## What lives in this workspace?

| Crate / folder        | Purpose                                                                                  |
|-----------------------|------------------------------------------------------------------------------------------|
| `src/dev_runner/`     | Validates flows by compiling each node’s describe() schema. |
| `crates/dev-viewer`   | Renders transcripts and highlights defaults/overrides so you can reason about configs.    |
| `greentic-dev component …` | Scaffolds, validates, and packs components (reusing the internal xtask tooling).    |
| `docs/`               | High-level guides (runner, mocks, viewer, scaffolder, developer guide).                  |
| `scripts/build_pages.py` | Builds the GitHub Pages site by combining Rustdoc output with the markdown guides.    |

You will also find mock-service helpers for HTTP, NATS, and vault-like secrets in the built-in runner modules, ready to be wired into flows.
For a hands-on, end-to-end walkthrough (component → pack → run), see `docs/developer-guide.md`.

---

## Install

From crates.io:

```bash
cargo install greentic-dev
```

Want faster installs from prebuilt release artifacts?

```bash
cargo install cargo-binstall
cargo binstall greentic-dev
```

`cargo binstall greentic-dev` installs the full companion tool bundle (including `greentic-component`, `packc`, `greentic-secrets`, and `greentic-gui`) so you don't need to install them separately.

Need the latest commit or working from a fork?

```bash
cargo install --git https://github.com/greentic-ai/greentic-dev greentic-dev
# or from the current checkout
cargo install --path .
```

> You do **not** need to clone this repository just to use the CLI—`cargo install greentic-dev` is all that’s required. Clone the repo only if you plan to contribute or hack on the tooling itself.

Once installed, `greentic-dev` becomes a single entry point for flow validation (`greentic-dev flow …`), deterministic pack builds (`greentic-dev pack …`), local pack runs, and component/MCP diagnostics.

> **Requirements**
>
> - Rust 1.89+ (the repo pins this via `rust-toolchain.toml`)
> - Component commands use the `greentic-component` crate in-process; install `greentic-component` if you want to invoke its CLI directly.
> - Pack commands use `packc` in-process; install `greentic-pack`/`packc` if you want the standalone CLI.
>
> Flow authoring: config flows now live inside `component.manifest.json` under `dev_flows`. If a flow is missing, run `greentic-component flow update` to regenerate config flows. `greentic-dev flow add-step` defaults to `--manifest ./component.manifest.json --flow default` and edits pack flows in `flows/<flow-id>.ygtc`.

---

## Quick start: validate → build → run

1. **Validate the flow schema**

   ```bash
   greentic-dev flow validate -f examples/flows/min.ygtc --json
   ```

   Prints the canonical `FlowBundle` (including the `hash_blake3`) so you can diff config changes or feed it into CI.

2. **Build a deterministic pack**

   ```bash
   greentic-dev pack build \
     -f examples/flows/min.ygtc \
     -o dist/demo.gtpack \
     --component-dir fixtures/components
   ```

   Uses the component resolver to fetch schemas/defaults, validates each node against component-provided describe payloads, and emits a `.gtpack` with stable hashes.

3. **Run the pack locally**

   ```bash
   greentic-dev pack run \
     -p dist/demo.gtpack \
     --mocks on \
     --allow api.greentic.dev
   ```

   Spins up the desktop runner with mocks, writes transcripts plus `run.json` under `.greentic/runs/<timestamp>/`, and prints the `RunResult` (status, node summaries, failures) to stdout. Add `--otlp <url>` or `--artifacts <dir>` to forward telemetry or keep outputs elsewhere.

Have an MCP provider to inspect? Enable the optional feature and run:

```bash
cargo run --features mcp -- mcp doctor fixtures/providers/dev
```

which validates a `toolmap.yaml` (or directory) and reports tool health before you wire nodes to it.

Registering provider extensions and inspecting them:

```bash
# add/update a provider entry inside PackManifest.extensions
greentic-dev pack new-provider --pack manifest.cbor \
  --id vendor.db \
  --runtime vendor.db.runtime::greentic_provider@greentic:provider/runtime \
  --manifest providers/vendor.db/provider.yaml \
  --kind database

# inspect provider extension contents in a built pack
greentic-pack providers list dist/demo.gtpack
greentic-pack providers info dist/demo.gtpack --id vendor.db
greentic-pack providers validate dist/demo.gtpack
```

---

## Why schema awareness matters

Flows in Greentic are YAML documents describing a set of nodes. Historically it was easy to typo a field or forget a required input; you would only discover the mistake at runtime. The runner in this repository flips that around:

1. Load your flow YAML.
2. For each node, call the component’s `describe()` (or use a registered schema stub).
3. Compile the JSON Schema (Draft 7) and validate the node configuration.
4. Merge defaults, capture resolved config, schema ID, and validation log in a transcript.

Because validation happens before execution, you can run it on every commit or as part of CI:

```bash
greentic-dev flow validate -f examples/flows/min.ygtc --json
```

The validation command is deliberately fast—it skips tool execution but still produces canonical JSON so you know exactly what would enter the runner.

> If you prefer not to install the CLI globally while developing, use `cargo run -p greentic-dev -- flow …` instead.

### Examining the transcript

Use the viewer to inspect the result:

```bash
cargo run -p dev-viewer -- --file .greentic/transcripts/min-<timestamp>.yaml
```

You will see output like:

```
inputs:
  client_id: abc (override)
  client_secret: null (default)
```

so you immediately know which fields rely on defaults versus user input.

---

## Cheatsheet: validate, view, iterate

| Action                          | Command                                                                 |
|---------------------------------|-------------------------------------------------------------------------|
| Validate a flow                 | `greentic-dev flow validate -f <flow>.ygtc [--json]`                    |
| Build a pack                    | `greentic-dev pack build -f <flow>.ygtc -o dist/out.gtpack`             |
| Run a pack locally              | `greentic-dev pack run -p dist/out.gtpack [--mocks on] [--allow host]`  |
| Verify a built pack             | `greentic-dev pack verify -p dist/out.gtpack [--policy strict|devok]`   |
| Init a pack from distributor    | `greentic-dev pack init --from pack://org/name@1.0.0 [--profile dev]`   |
| Scaffold a pack workspace       | `greentic-dev pack new -- --name demo-pack` *(delegated to packc)*      |
| Register a provider extension   | `greentic-dev pack new-provider --pack manifest.cbor --id vendor.db --runtime vendor.db.runtime::greentic_provider@greentic:provider/runtime [--kind <cap>] [--manifest providers/vendor.db/provider.yaml]` |
| Inspect pack providers          | `greentic-pack providers list dist/out.gtpack` *(try `info` / `validate` too)* |
| View transcript                 | `cargo run -p dev-viewer -- --file .greentic/transcripts/<file>.yaml`   |
| Scaffold a component            | `greentic-dev component new <name>`                                     |
| Add a remote component          | `greentic-dev component add component://org/name@^1.0 [--profile dev]`  |
| Build + doctor a component      | `greentic-dev component build --manifest <component.manifest.json>` + `greentic-dev component doctor <wasm> --manifest <component.manifest.json>` |
| Pack a component                | `greentic-dev component pack --path <dir>`                              |
| List component templates        | `greentic-dev component templates --json`                               |
| Scaffold with org defaults      | `greentic-dev component new --name echo --org ai.greentic`              |
| Doctor a component artifact     | `greentic-dev component doctor <path/to/component.wasm> --manifest <component.manifest.json>` |
| Set default org/template        | `greentic-dev config set defaults.component.org ai.greentic`            |
| Serve GUI packs locally         | `greentic-dev gui serve [--config <path>]`                              |
| Stage a GUI dev pack            | `greentic-dev gui pack-dev --dir <assets> --output <pack-dir>`          |
| Inspect MCP tool map (feature)  | `greentic-dev mcp doctor <toolmap>`                                     |
| Run full test suite             | `cargo test`                                                            |
| Lint everything                 | `cargo clippy --all-targets --all-features -- -D warnings`              |
| Format                          | `cargo fmt`                                                             |

Need to exercise only the component integration tests? Use `make itests`—it automatically skips when `greentic-component` is not on your `PATH`.
_Component commands above delegate to the `greentic-component` CLI, so new subcommands or flags are available here as soon as they land upstream._

### Distributor profiles (for `component add` / `pack init`)

Config search order (first existing wins): `GREENTIC_DEV_CONFIG_FILE` → `GREENTIC_CONFIG_FILE` → `GREENTIC_CONFIG` → `$XDG_CONFIG_HOME/greentic-dev/config.toml` → `$HOME/.config/greentic-dev/config.toml` → `$HOME/.greentic/config.toml`. The loaded path and search list are reported in errors.

Prefer the profile map form:

```toml
[distributor]
default_profile = "default" # or set GREENTIC_DISTRIBUTOR_PROFILE/--profile

[distributor.profiles.default]
base_url = "https://distributor.greentic.cloud"
token = "env:GREENTIC_TOKEN" # resolved via environment variable
tenant_id = "prod"
environment_id = "prod"

[distributor.profiles.dev]
base_url = "http://localhost:7070"
token = ""
tenant_id = "dev"
environment_id = "dev"
```

Inline defaults are also supported:

```toml
[distributor]
default_profile = { name = "inline", base_url = "http://localhost:7070", tenant_id = "dev", environment_id = "dev" }
```

Select a profile with `--profile <name>` or `GREENTIC_DISTRIBUTOR_PROFILE`.
Legacy `[distributor.<name>]` tables remain supported; they are merged with `distributor.profiles`.

For a deeper, example-driven walkthrough, see `docs/distributor.md`.

---

## GUI dev workflow (packs + greentic-gui)

`greentic-dev gui serve` runs greentic-gui against a local set of GUI packs. It discovers config in this order: `./gui-dev.yaml`, `./.greentic/gui-dev.yaml`, `~/.config/greentic-dev/gui-dev.yaml`. Defaults: bind `127.0.0.1:8080`, domain `localhost:8080`. greentic-gui is spawned from `--gui-bin`, `greentic-gui` on PATH, or `cargo run -p greentic-gui`.

Example `gui-dev.yaml`:

```yaml
tenant: did:web:example
domain: localhost:8080
layout_pack: ./packs/layout
auth_pack: ./packs/auth
skin_pack: ./packs/skin
feature_packs:
  - ./packs/billing
env:
  LOG_LEVEL: debug
```

`greentic-dev gui pack-dev` stages a minimal GUI pack from static assets:

```
greentic-dev gui pack-dev --dir dist --output ./packs/layout --kind layout --entrypoint index.html
greentic-dev gui pack-dev --dir dist --output ./packs/billing --kind feature --feature-route /invoices --feature-html invoices.html
```

Use `--manifest` to supply a custom manifest or `--build-cmd`/`--no-build` to control local builds. Output must be empty; assets are copied to `output/gui/assets`, and a manifest is generated if not provided.

---

## Creating a component – the “why” and the “how”

Below is the workflow we follow when creating a new component that we can validate and iterate locally. Each step highlights **why** it matters inside the Greentic ecosystem.

### 1. Scaffold with `greentic-dev component`

```bash
greentic-dev component templates --json | jq '.[0]'
greentic-dev component new my-component --org ai.greentic
cd component-my-component
```

**Why**: The scaffold wires up provider metadata, a `greentic-interfaces-guest` hello world, and a sensible default manifest so you can build immediately without vendoring WIT.

Generated layout:

```
component-my-component/
├── Cargo.toml
├── provider.toml
├── README.md
├── schemas/v1/config.schema.json
└── src/lib.rs
```

### 2. Model the configuration schema

Edit `schemas/v1/config.schema.json` with the fields and defaults your node exposes. The runner uses this schema to validate flows and merge defaults into transcripts, so keep it authoritative. Document the same contract in the component’s `README.md` (or an internal `docs/` folder) for flow authors.

### 3. Implement behaviour in `src/lib.rs`

The template already exports `greentic:component/node` and echoes a `message`, while calling into the guest crates for secrets/state/HTTP/telemetry. Replace the stub with real logic and import any extra guest modules you need (e.g., OAuth broker, lifecycle). Update `provider.toml` whenever capabilities, versions, or artifact paths change.

### 4. Build and doctor

```bash
cargo component build --release --target wasm32-wasip2
greentic-dev component build --manifest component.manifest.json
greentic-dev component doctor ./target/wasm32-wasip2/release/<name>.wasm --manifest component.manifest.json
```

**Why**: `cargo component` produces a Preview 2 component (`wasm32-wasip2`) using the published guest bindings, keeping builds reproducible without bundling local WIT. `greentic-dev component build` refreshes config flows/schemas and hashes; `component doctor` confirms the artifact/manifest agree and inspects exports. When the manifest isn’t next to the artifact, pass `--manifest <path>`.

### 5. Package for distribution (optional)

```bash
greentic-dev component pack --path .
greentic-dev pack new -- --name hello-pack    # delegated to packc
```

Creates `packs/my-component/0.1.0/` with the `.wasm`, `meta.json` (provider metadata + SHA + timestamp), and `SHA256SUMS`. Use this output when publishing or handing the component to downstream teams.

### 6. Wire into flows and inspect transcripts

Back in the main workspace:

```bash
greentic-dev flow validate -f examples/flows/my-component.ygtc --json
greentic-dev pack build -f examples/flows/my-component.ygtc -o dist/my-component.gtpack
greentic-dev pack run -p dist/my-component.gtpack --mocks on
```

The validation/build steps ensure the flow matches the schema and the pack stays deterministic; the runner writes transcripts/`run.json` so you can review defaults vs overrides. Use the mock services (`docs/mocks.md`) to emulate HTTP/NATS/secret providers while you iterate, and point the viewer at `.greentic/runs/<timestamp>/transcript.jsonl` (or the YAML artifacts written by older flows) for a detailed walkthrough.

---

Before opening a PR, keep the usual guardrails clean:

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

When Greentic interface versions update, bump the guest crate version in the scaffolder and regenerate as needed so the bindings and provider metadata stay aligned.

* Rust API docs (`cargo doc` output),
* Runner, mocks, viewer, scaffolder guides, and
* The developer guide (this document) so the process is documented once.

Finally, publish your component’s own schema (usually under `component-<name>/gh-pages`) so the runner can fetch it in describe() responses.

---

## Additional resources

* **Runner guide** – `docs/runner.md`
* **Mocks guide** – `docs/mocks.md`
* **Viewer guide** – `docs/viewer.md`
* **Scaffolder internals** – `docs/scaffolder.md`
* **Developer guide (HTML)** – `https://greentic-ai.github.io/greentic-dev/docs/developer-guide.html`
* **GitHub Pages index** – `https://greentic-ai.github.io/greentic-dev/`

## CLI reference

All commands are available both through the installed binary (`greentic-dev …`) and via `cargo run -p greentic-dev -- …` while developing locally.

```
greentic-dev flow validate -f <flow.ygtc> [--json]

greentic-dev pack build -f <flow.ygtc> -o <out.gtpack>
                        [--sign dev|none] [--meta pack.toml]
                        [--component-dir DIR]

greentic-dev pack run -p <pack.gtpack>
                      [--entry FLOW] [--input JSON]
                      [--policy strict|devok]
                      [--otlp URL] [--allow host[,..]]
                      [--mocks on|off] [--artifacts DIR]

greentic-dev component [<ARGS>...]
                      # delegates directly to the `greentic-component` CLI

greentic-dev mcp doctor <toolmap|provider> [--json]    # feature = "mcp"
```

## Local CI checks

Run the same steps that CI executes:

```bash
ci/local_check.sh
```

It enforces `cargo fmt`, `cargo clippy --all-features`, `cargo build --workspace --all-features --locked`, and `cargo test --workspace --all-features --locked -- --nocapture`. The script sets up isolated `CARGO_HOME`/`CARGO_TARGET_DIR` just like CI, so if it passes locally, the workflow will pass as well.

- **`run`**: Compile each node schema and validate a flow YAML. `--print-schemas` lists registry stubs. `--validate-only` skips execution (flow execution is still under development).
- **`component …`**: Every invocation is forwarded to the `greentic-component` CLI, so any new subcommands or flags shipped there are immediately available here.

Happy building! This toolkit should make it painless to iterate on components with confidence before they enter the main platform.
