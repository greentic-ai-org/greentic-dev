# Plan: Move Flow Semantics from greentic-dev into greentic-flow

## Candidate PR 1: Normalize manifests and config flows inside greentic-flow
- **Scope:** Accept legacy `operations: ["echo"]` shapes and missing/invalid `type` in dev/config flows directly in greentic-flow helpers.
- **API changes:**  
  - Add a manifest normalization step in `greentic_flow` (or `greentic_types`) that upgrades string operations to `{ name }` objects before parsing.  
  - Extend `config_flow` loader to inject `type: component-config` when absent/non-string, so consumers need not patch graphs.
- **Tests to add (greentic-flow):**  
  - Manifest parsing test where `operations` is an array of strings.  
  - Config flow load/validate test with missing `type` still passes after normalization.  
- **Acceptance criteria:**  
  - CLI/library callers can remove `normalize_manifest` and `render_config_flow_yaml` logic and rely on upstream behavior.  
  - Backward-compat manifests/config flows load without caller intervention.

## Candidate PR 2: Provide a first-class config-flow runner + add-step entrypoint
- **Scope:** Expose an API in greentic-flow to execute dev/config flows and return `{ node_id, node }`, paired with a convenience to run `plan_add_step`/`apply_and_validate`.
- **API changes:**  
  - Add `run_config_flow` helper that runs a config flow file (or in-memory graph) via the greentic-flow stack (not greentic-dev runner) and returns normalized node output.  
  - Add a helper `add_step_from_config_flow(flow_doc, config_flow_path, manifest_paths, after, allow_cycles)` that wraps: load pack flow → run config flow → build catalog → plan/apply.
- **Tests to add (greentic-flow):**  
  - Integration test that runs a config flow emitting `component.exec` and applies add-step to a pack flow, asserting routing/threading.  
  - Failure tests for legacy `tool` emissions and missing operations.
- **Acceptance criteria:**  
  - greentic-dev can delegate config-flow execution and add-step orchestration to greentic-flow without its own runner glue.  
  - Outputs/errors match current greentic-flow add-step diagnostics.

## Candidate PR 3: Shared component catalog construction
- **Scope:** Provide a reusable way to build `ManifestCatalog` from multiple manifests or a pack manifest, ensuring required-field validation matches pack contents.
- **API changes:**  
  - Add `ManifestCatalog::load_from_pack_manifest(pack_manifest_path)` or `load_from_manifest_dir(dir)` to aggregate all components.  
  - Optionally expose a builder to merge multiple manifest paths.
- **Tests to add (greentic-flow):**  
  - Catalog resolves required fields when multiple manifests are present.  
  - Add-step validation fails when required fields are missing in any component referenced in the pack flow.
- **Acceptance criteria:**  
  - greentic-dev can drop its single-manifest catalog logic and rely on upstream aggregation for add-step validation.

## Candidate PR 4: Expose cycle policy in add-step API/CLI
- **Scope:** Let callers control `allow_cycles` instead of greentic-dev hardcoding `false`.
- **API changes:**  
  - Document and surface `allow_cycles` in greentic-flow CLI/entrypoints (if desired), keeping the default `false` but making it explicit.  
  - Consider an enum for routing policy if more modes are added.
- **Tests to add (greentic-flow):**  
  - Add-step test that succeeds/fails based on `allow_cycles` toggled.  
  - Ensure diagnostics are clear when cycles are rejected.
- **Acceptance criteria:**  
  - greentic-dev can simply pass through user intent, and defaults remain centralized in greentic-flow.

## Candidate PR 5: Optional UX helpers (prompting/anchors)
- **Scope:** Offer optional, non-interactive helpers in greentic-flow for choosing anchors, so CLI layers don’t need to duplicate logic.
- **API changes:**  
  - Add a utility to list valid anchor nodes (entrypoints-first) for interactive clients.  
  - No behavior change; purely data to drive prompts.
- **Tests to add (greentic-flow):**  
  - Unit test listing anchors for simple graphs; entrypoint ordering preserved.
- **Acceptance criteria:**  
  - greentic-dev keeps prompts but can drop custom sorting/selection logic if desired.
