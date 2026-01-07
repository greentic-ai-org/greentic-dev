# Plan: Move Component Semantics into greentic-component (or shared resolver)

## Scope (semantics-focused)
- Preparing components from disk, version gating, describe/schema selection (latest describe), JSON Schema validation.
- Node payload extraction and validation with clear pointers.
- Namespace short-name fallback when resolving under `--component-dir`.
- Coordinate parsing semantics (ids + semver intent) beyond transport.
- Workspace manifest structure/update (if serving as component catalog).

## Candidate PRs

### PR 1: Shared Component Resolver API
- **API:** `prepare_and_validate(target, version_req, component_dir_opt) -> PreparedComponentSemantics`
  - Handles short-name fallback (`ns.foo` → `foo` under component_dir).
  - Calls `prepare_component`, checks semver requirement, selects latest describe schema.
  - Exposes `validate_payload(node_id, component_id, payload) -> Vec<Error>` with JSON Schema pointers.
- **Tests:** version mismatch errors; short-name fallback success; schema validation surfaces instance paths; caches describe/schema as needed.
- **Acceptance:** greentic-dev can drop `component_resolver.rs` bespoke prep/validation and use this API.

### PR 2: Workspace Manifest Updater
- **API:** helper to upsert `.greentic/manifest.json` (or equivalent) with `ComponentEntry` from a cached artifact path.
- **Behavior:** replace entry when component id matches, preserve others, optional hash calculation.
- **Tests:** insert/replace scenarios; malformed manifest handled gracefully.
- **Acceptance:** greentic-dev can delete `update_manifest` and rely on upstream helper.

### PR 3: Coordinate Semantics Helper (optional)
- **API:** parse component coordinate for semantic use (id + semver requirement), distinct from transport parsing.
- **Behavior:** last-`@` split with default `*`; optionally validate against allowed namespaces.
- **Tests:** no-`@` → `*`; malformed semver error.
- **Acceptance:** greentic-dev can stop re-parsing coordinates for semantic checks.

### PR 4: Describe/Schema Cache Strategy (optional)
- **Scope:** Centralize cache of compiled JSON Schemas and describe selection to avoid per-tool duplication.
- **Tests:** cache hit behavior; invalid schema error clarity.
- **Acceptance:** greentic-dev can remove local schema cache and rely on shared cache.

## Classification
- MUST MOVE: prep + version gating + describe/schema selection + payload validation; short-name fallback; manifest updater; semantic coordinate parsing.
- MAY STAY: CLI UX/logging around installs; local-path short-circuit (could also live upstream).
- DELETE after migration: local schema compile cache and describe selection in greentic-dev.
