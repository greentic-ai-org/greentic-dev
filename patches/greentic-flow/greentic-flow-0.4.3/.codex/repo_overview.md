# Repository Overview

## 1. High-Level Purpose
- Rust crate `greentic-flow` that defines a JSON Schema for YGTC flows, loads and validates YAML flows against it, and converts them into a compact intermediate representation with component metadata and routing.
- Provides two CLIs: `greentic-flow` scaffolds new flow files (optionally updating pack manifests), and `ygtc-lint` validates/lints flows (including adapter registry checks) with JSON-friendly output for tooling.

## 2. Main Components and Functionality
- **Path:** `src/loader.rs`  
  **Role:** Parse YAML flows, validate against the JSON schema, and normalise nodes.  
  **Key functionality:** Reads schema from disk; converts YAML to JSON for schema validation; enforces exactly one component key per node plus optional `routing`; validates component key regex; expands routing objects; checks referenced nodes exist; defaults `start` to `in` if absent and node exists.  
  **Key dependencies / integration points:** Uses `schemas/ygtc.flow.schema.json`; feeds into IR conversion and bundle building.
- **Path:** `src/flow_bundle.rs`  
  **Role:** Build canonicalised `FlowBundle` artifacts with hashes and component pins.  
  **Key functionality:** Embeds the schema for validation convenience; canonicalises JSON ordering; computes BLAKE3 hashes; derives entry node (prefers explicit `start`, then `in`, then first node); extracts component pins with wildcard version requirements; returns bundles alongside IR when requested.
- **Path:** `src/ir.rs`  
  **Role:** Define intermediate representation and component classification helpers.  
  **Key functionality:** `FlowIR`/`NodeIR` structures retain parameters, payload expressions, and routing; `classify_node_type` distinguishes adapter-style components (`<namespace>.<adapter>.<operation>`) from builtins.
- **Path:** `src/util.rs`  
  **Role:** Shared helpers.  
  **Key functionality:** Component key validation now allows both namespaced components and builtin helpers (`questions`, `template`) used by config flows.
- **Path:** `src/resolve.rs`  
  **Role:** Resolve parameter references inside payload expressions.  
  **Key functionality:** Recursively walks JSON values and replaces `parameters.*` string references; errors when paths are missing or non-object.
- **Path:** `src/registry.rs`  
  **Role:** Adapter catalog loader and lookup helper.  
  **Key functionality:** Loads registries from JSON (or TOML when the `toml` feature is enabled); checks for known adapter operations via `contains`.
- **Path:** `src/lint` (including `adapter_resolvable.rs`, `mod.rs`)  
  **Role:** Flow linting rules.  
  **Key functionality:** `AdapterResolvableRule` ensures adapter nodes exist in the registry; builtin lint checks include validation that declared `start` nodes exist.  
  **Key dependencies / integration points:** Operates on `FlowIR` and optional `AdapterCatalog`.
- **Path:** `src/json_output.rs`  
  **Role:** Map errors/lint results to machine-readable diagnostics.  
  **Key functionality:** Converts `FlowError` into structured diagnostics with pointers; emits JSON payloads for lint success/failure; used by CLI and helper APIs.
- **Path:** `src/config_flow.rs`  
  **Role:** Lightweight harness for executing config flows.  
  **Key functionality:** Runs flows containing `questions` and `template` components, seeds state from answers/defaults, renders final template into `{ node_id, node }` output for insertion elsewhere.
- **Path:** `src/bin/ygtc-lint.rs`  
  **Role:** CLI for schema validation and linting.  
  **Key functionality:** Supports file/dir recursion and stdin (`--json --stdin`); loads schema text once; optional adapter registry; prints human-readable results or JSON contract including bundle/hash.  
  **Key dependencies / integration points:** Relies on loader/bundle/lint modules and telemetry macro from `greentic-types`.
- **Path:** `src/bin/greentic-flow.rs`  
  **Role:** CLI scaffolder for new `.ygtc` flows.  
  **Key functionality:** Generates messaging/events/deployment templates; infers flow IDs from paths; optional descriptions; writes files with `--force` handling; reads pack manifest (default `manifest.yaml`) to infer deployment kind, warn on mismatches, and append new flows with relative paths.
- **Path:** `schemas/ygtc.flow.schema.json`  
  **Role:** JSON Schema defining flow structure.  
  **Key functionality:** Requires `id`, `type`, and `nodes`; allows known kinds plus arbitrary custom strings (e.g., `component-config`); enforces single component key plus optional routing per node; validates component name patterns and includes shapes for builtin `questions` and `template` components.
- **Path:** `fixtures/`, `examples/`, `tests/`  
  **Role:** Sample flows, golden bundles, and integration/unit tests covering loading, IR conversion, hashing, adapter linting, CLI scaffolding, and JSON output contracts; config-flow fixture (`tests/data/config_flow.ygtc`) and harness test (`tests/config_flow.rs`) exercise the config-flow convention.
- **Path:** `docs/deployment-flows.md`  
  **Role:** Guidance on treating deployment flows as events-based graphs and using the scaffolder for deployment templates.

## 3. Work In Progress, TODOs, and Stubs
- No active stubs or TODO markers noted in code; builtin lint now checks for missing start nodes.

## 4. Broken, Failing, or Conflicting Areas
- **Location:** Repository tests (`cargo test`)  
  **Evidence:** Attempted `cargo test` twice; both runs timed out while blocking on the Cargo build directory file lock.  
  **Likely cause / nature of issue:** Another process may be holding the `target` lock; test status currently unknown until the lock clears.

## 5. Notes for Future Work
- Re-run the full test suite once the Cargo build lock clears to confirm current health.
- Consider extending the config-flow harness to support richer components beyond `questions`/`template`.
- Add more builtin lint coverage (e.g., routing sanity, duplicate node detection) as needed.
