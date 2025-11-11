# greentic-dev – schema-aware developer toolkit

`greentic-dev` is the command-line toolbox we use to design, validate, and iterate on Greentic components before they ever hit production. It bundles a schema-aware flow runner, mock services, a transcript viewer, and a component scaffolder into one workspace so that building new automation feels repeatable and safe.

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
| `src/dev_runner/`     | Validates flows by compiling each node’s describe() schema and optional conformance kit. |
| `crates/dev-viewer`   | Renders transcripts and highlights defaults/overrides so you can reason about configs.    |
| `greentic-dev component …` | Scaffolds, validates, and packs components (reusing the internal xtask tooling).    |
| `docs/`               | High-level guides (runner, mocks, viewer, scaffolder, developer guide).                  |
| `scripts/build_pages.py` | Builds the GitHub Pages site by combining Rustdoc output with the markdown guides.    |

You will also find mock-service helpers for HTTP, NATS, and vault-like secrets in the built-in runner modules, ready to be wired into flows.

---

## Install

From crates.io:

```bash
cargo install greentic-dev
```

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
> - Rust 1.88+ (the repo pins this via `rust-toolchain.toml`)
> - The component subcommands delegate to the `greentic-component` CLI. Install `greentic-component >= 0.3.2` (for example `cargo install greentic-component --force --version 0.3`) so `greentic-dev component new/templates/doctor` can run. You can also point to a custom binary and set defaults via `~/.greentic/config.toml`:
>
> ```toml
> [tools.greentic-component]
> path = "/opt/bin/greentic-component"
>
> [defaults.component]
> org = "ai.greentic"
> template = "rust-wasi-p2-min"
> ```
>
> Environment variables such as `GREENTIC_TEMPLATE_ROOT` and `GREENTIC_TEMPLATE_YEAR` are forwarded automatically, and you can opt into telemetry reporting by adding `--telemetry` to the component subcommands.

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

---

## Why schema awareness matters

Flows in Greentic are YAML documents describing a set of nodes. Historically it was easy to typo a field or forget a required input; you would only discover the mistake at runtime or during a conformance run. The runner in this repository flips that around:

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
| View transcript                 | `cargo run -p dev-viewer -- --file .greentic/transcripts/<file>.yaml`   |
| Scaffold a component            | `greentic-dev component new <name>`                                     |
| Validate a component            | `greentic-dev component validate --path <dir>`                          |
| Pack a component                | `greentic-dev component pack --path <dir>`                              |
| List component templates        | `greentic-dev component templates --json`                               |
| Scaffold with org defaults      | `greentic-dev component new --name echo --org ai.greentic`              |
| Doctor a component workspace    | `greentic-dev component doctor --path ./echo` *(delegated)*             |
| Set default org/template        | `greentic-dev config set defaults.component.org ai.greentic`            |
| Inspect MCP tool map (feature)  | `greentic-dev mcp doctor <toolmap>`                                     |
| Run full test suite             | `cargo test` \| `cargo test --features conformance`                     |
| Lint everything                 | `cargo clippy --all-targets --all-features -- -D warnings`              |
| Format                          | `cargo fmt`                                                             |

Need to exercise only the component integration tests? Use `make itests`—it automatically skips when `greentic-component` is not on your `PATH`.
_Component commands above delegate to the `greentic-component` CLI, so new subcommands or flags are available here as soon as they land upstream._

---

## Creating a component – the “why” and the “how”

Below is the workflow we follow when creating a new component that we can validate and iterate locally. Each step highlights **why** it matters inside the Greentic ecosystem.

### 1. Scaffold with `greentic-dev component`

```bash
greentic-dev component templates --json | jq '.[0]'
greentic-dev component new my-component --org ai.greentic
cd component-my-component
```

**Why**: The scaffold wires up provider metadata, vendored WIT packages, and a `wit_bindgen` hello world so you can build immediately without chasing dependencies.

Generated layout:

```
component-my-component/
├── Cargo.toml
├── provider.toml
├── README.md
├── schemas/v1/config.schema.json
├── src/lib.rs
└── wit/
    ├── world.wit
    └── deps/
        ├── greentic-component-<ver>/
        ├── greentic-host-import-<ver>/
        └── greentic-types-core-<ver>/
```

### 2. Model the configuration schema

Edit `schemas/v1/config.schema.json` with the fields and defaults your node exposes. The runner uses this schema to validate flows and merge defaults into transcripts, so keep it authoritative. Document the same contract in the component’s `README.md` (or an internal `docs/` folder) for flow authors.

### 3. Implement behaviour in `src/lib.rs`

The template already exports `greentic:component/node` and echoes a `message`. Replace the stub with real logic. If you need additional WIT packages, drop them under `wit/deps/` and add a line to `Cargo.toml`’s `package.metadata.component.target.dependencies`. Update `provider.toml` whenever capabilities, versions, or artifact paths change.

### 4. Build and validate

```bash
cargo component build --release --target wasm32-wasip2
greentic-dev component validate --path .
```

**Why**: `cargo component` produces a Preview 2 component (`wasm32-wasip2`) using only the vendored WIT, which keeps builds reproducible. `greentic-dev component validate` confirms the artifact and metadata agree (WIT package IDs, world name, version pins) and, when WASI shims exist, inspects the manifest via the component runtime. If WASI support is missing locally, validation still passes but prints a warning that manifest inspection was skipped.

### 5. Package for distribution (optional)

```bash
greentic-dev component doctor --path .
greentic-dev component pack --path .
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

When Greentic interface versions update, re-vendor the WIT under `wit/deps/` (re-run the scaffold or copy from the cargo registry) and adjust `provider.toml` + `Cargo.toml` to match. This ensures validation continues to run purely against published crates.

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

If you need help wiring your component into the larger conformance suites, check the `greentic-conformance` crate (available on crates.io) and wire its flows into the `greentic-dev` runner APIs.

---

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
