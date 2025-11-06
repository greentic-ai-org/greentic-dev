# Component Scaffolder (`cargo xtask new-component`)

The scaffolder now emits a lean component workspace that builds with `cargo component`, ships its own WIT dependencies, and exposes provider metadata out of the box. This guide walks through what the scaffold contains and how to iterate on it.

---

## Generated layout

```
component-<name>/
├── Cargo.toml
├── provider.toml
├── README.md
├── schemas/
│   └── v1/config.schema.json
├── src/
│   └── lib.rs
└── wit/
    ├── world.wit
    └── deps/
        ├── greentic-component-<ver>/
        ├── greentic-host-import-<ver>/
        └── greentic-types-core-<ver>/
```

### Key files

- **`Cargo.toml`** – Minimal manifest that depends on `wit-bindgen` 0.44, `serde`, and `serde_json`, and configures `package.metadata.component` so `cargo component build` targets the vendored `wit/` directory.
- **`provider.toml`** – Canonical metadata (name, version, ABI pins, capabilities, artifact location). `xtask validate` and `xtask pack` both consume this file.
- **`README.md`** – Quickstart for the component author (build, validate, pack).
- **`schemas/v1/config.schema.json`** – Draft 7 JSON Schema for the node configuration used by the runner and transcripts.
- **`src/lib.rs`** – Hello-world implementation generated via `wit_bindgen::generate!`. It exports the `greentic:component/node` world and echoes an input `message`.
- **`wit/world.wit`** – Declares the component’s world and imports the Greentic schema packages.
- **`wit/deps/*`** – Vendored WIT packages resolved from the Greentic interfaces version currently pinned in the main workspace `Cargo.lock`. These keep component builds offline and deterministic.

Older assets (`src/describe.rs`, `tests/schema_validates_examples.rs`, `examples/flows/min.yaml`, `.github/workflows/ci.yml`) are intentionally no longer generated; they live in the main repository instead.

---

## Typical workflow inside the scaffold

1. **Build:** `cargo component build --release`  
   Compiles to `target/wasm32-wasip1/release/<name>.wasm`. The scaffolder sets `CARGO_COMPONENT_CACHE_DIR` to a local folder so the command works offline once the cargo cache is warmed.

2. **Validate:** `cargo run -p xtask -- validate --path .`  
   Checks that the artifact exists, decodes WIT metadata, compares it against `provider.toml`, and (if WASI host shims are present) inspects the manifest via `component-runtime`. Missing WASI support produces a warning but doesn’t fail validation.

3. **Pack (optional):** `cargo run -p xtask -- pack --path .`  
   Copies the `.wasm`, writes `meta.json` (provider metadata + sha + timestamp), and generates `SHA256SUMS` under `packs/<name>/<version>/`.

4. **Wire into flows:** Back in the main workspace, point a flow node at the component (`using: <name>`) and run `cargo run -p greentic-dev -- run -f <flow>.yaml --validate-only`.

---

## Customising the scaffold

- **Schema & defaults:** Edit `schemas/v1/config.schema.json` and keep it in sync with the behaviour inside `src/lib.rs`. Greentic transcripts record defaults vs overrides directly from this schema.
- **Provider metadata:** Update `provider.toml` as your component evolves (capabilities, WIT package requirements, artifact path).
- **Additional WIT dependencies:** If your component needs more Greentic or WASI interfaces, vendor them under `wit/deps/` and reference them in `Cargo.toml` via `package.metadata.component.target.dependencies`.
- **Documentation:** Extend the scaffolded `README.md` or add a `docs/` directory in the component repo to mirror the patterns we use across Greentic components.

---

## FAQ

**Why does `xtask validate` sometimes skip manifest exports?**  
Until the runtime bundles WASI Preview 2 shims, components that import `wasi:*` interfaces cannot be instantiated locally. The validator spots this case, prints a warning, and finishes without the runtime checks.

**Can I regenerate WIT deps when Greentic interfaces upgrade?**  
Yes. Update the Greentic workspace to the new crate versions, rerun `cargo run -p xtask -- new-component ...` for a fresh scaffold, or manually re-vendor the directories and adjust `provider.toml`.

**Where did the old `describe.rs` go?**  
The WASM component now exposes `describe` capabilities directly through `wit_bindgen` exports. The CLI runner prefers the schema that comes from the component artifact, so no extra stub is needed in the scaffold.

---

Use this document any time you scaffold a new component to ensure you understand the generated pieces and how they factor into validation.
