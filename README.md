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
| `crates/dev-runner`   | Validates flows by compiling each node’s describe() schema and optional conformance kit. |
| `crates/dev-viewer`   | Renders transcripts and highlights defaults/overrides so you can reason about configs.    |
| `greentic-dev component …` | Scaffolds, validates, and packs components (reusing the internal xtask tooling).    |
| `docs/`               | High-level guides (runner, mocks, viewer, scaffolder, developer guide).                  |
| `scripts/build_pages.py` | Builds the GitHub Pages site by combining Rustdoc output with the markdown guides.    |

You will also find mock-service helpers for HTTP, NATS, and vault-like secrets in `dev-runner`, ready to be wired into flows.

---

## Install

From a local checkout:

```bash
cargo install --path .
```

Or pull straight from the repository:

```bash
cargo install --git https://github.com/greentic-ai/greentic-dev greentic-dev
```

Once installed, `greentic-dev` becomes a single entry point for both flow validation (`greentic-dev run …`) and component tooling (`greentic-dev component …`).

---

## Why schema awareness matters

Flows in Greentic are YAML documents describing a set of nodes. Historically it was easy to typo a field or forget a required input; you would only discover the mistake at runtime or during a conformance run. The runner in this repository flips that around:

1. Load your flow YAML.
2. For each node, call the component’s `describe()` (or use a registered schema stub).
3. Compile the JSON Schema (Draft 7) and validate the node configuration.
4. Merge defaults, capture resolved config, schema ID, and validation log in a transcript.

Because validation happens before execution, you can run it on every commit or as part of CI:

```bash
greentic-dev run -f examples/flows/min.yaml --validate-only
```

The `--validate-only` flag is deliberately fast—it skips tool execution but still produces transcripts at `.greentic/transcripts/`.

> If you prefer not to install the CLI globally while developing, use `cargo run -p greentic-dev -- run …` instead.

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
| Validate a flow                 | `greentic-dev run -f <flow>.yaml --validate-only`                       |
| View transcript                 | `cargo run -p dev-viewer -- --file .greentic/transcripts/<file>.yaml`   |
| Scaffold a component            | `greentic-dev component new <name>`                                     |
| Validate a component            | `greentic-dev component validate --path <dir>`                          |
| Pack a component                | `greentic-dev component pack --path <dir>`                              |
| Run full test suite             | `cargo test` \| `cargo test --features conformance`                     |
| Lint everything                 | `cargo clippy --all-targets --all-features -- -D warnings`              |
| Format                          | `cargo fmt`                                                             |

---

## Creating a component – the “why” and the “how”

Below is the workflow we follow when creating a new component that we can validate and iterate locally. Each step highlights **why** it matters inside the Greentic ecosystem.

### 1. Scaffold with `greentic-dev component`

```bash
greentic-dev component new my-component
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
greentic-dev component pack --path .
```

Creates `packs/my-component/0.1.0/` with the `.wasm`, `meta.json` (provider metadata + SHA + timestamp), and `SHA256SUMS`. Use this output when publishing or handing the component to downstream teams.

### 6. Wire into flows and inspect transcripts

Back in the main workspace:

```bash
greentic-dev run -f examples/flows/my-component.yaml --validate-only
cargo run -p dev-viewer -- --file .greentic/transcripts/<file>.yaml
```

The runner ensures the flow matches the schema; the viewer shows which values came from defaults versus overrides so you can spot configuration drift quickly. Use the mock services (`docs/mocks.md`) to emulate HTTP/NATS/secret providers while you iterate.

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

If you need help wiring your component into the larger conformance suites, check the `greentic-conformance` crate (available on crates.io) and wire its flows into `dev-runner`’s validation APIs.

---

## CLI reference

All commands are available both through the installed binary (`greentic-dev …`) and via `cargo run -p greentic-dev -- …` while developing locally.

```
greentic-dev run -f <flow.yaml> [--validate-only] [--print-schemas]

greentic-dev component new <name> [--dir DIR]
greentic-dev component validate [--path PATH] [--skip-build]
greentic-dev component pack [--path PATH] [--out-dir DIR] [--skip-build]
greentic-dev component demo-run [--path PATH] [--artifact FILE]
                               [--operation NAME] [--input JSON]
                               [--config FILE] [--skip-build]
```

- **`run`**: Compile each node schema and validate a flow YAML. `--print-schemas` lists registry stubs. `--validate-only` skips execution (flow execution is still under development).
- **`component new`**: Scaffold a component in the current directory (or `--dir`). Generates provider metadata, vendored WIT, schema, and README.
- **`component validate`**: Ensure the built artifact matches `provider.toml` (WIT package IDs, world identifier). Rebuilds unless `--skip-build` is supplied.
- **`component pack`**: Produce `packs/<name>/<version>/` with the `.wasm`, `meta.json`, and SHA256 sums. Ideal for distribution.
- **`component demo-run`**: Load the component through `component-runtime`, apply configuration, and invoke an operation locally for quick end-to-end smoke tests.

Happy building! This toolkit should make it painless to iterate on components with confidence before they enter the main platform.
