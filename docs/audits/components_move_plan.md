# Plan: Move Component Resolution Semantics out of greentic-dev

## Candidate PR A (greentic-distributor-client): Coordinate/resolve/cache helper
- **Scope:** Provide a high-level “resolve component coordinate” API that handles offline/stub, coordinate parsing, resolve + download, and workspace cache updates.
- **API changes:**  
  - Accept coordinates with optional `@<semver>` (default `*`).  
  - Offline/stub hooks (env or explicit) to bypass network.  
  - Return cache path + metadata; optionally write workspace manifest entry.  
  - Support http(s)/file:// artifacts; return clear errors for unsupported OCI/internal until implemented.
- **Tests:**  
  - Offline without stub → error; offline with stub JSON → success.  
  - Coordinate without `@` → defaults to `*`.  
  - http(s) and file:// artifact fetch.  
  - Workspace manifest updated with entry for the resolved component.
- **Acceptance:** greentic-dev can delete `component_add.rs` logic and call the new API.

## Candidate PR B (greentic-distributor-client): Cache layout + slugging policy
- **Scope:** Standardize cache base and slug derivation so CLI callers don’t reimplement.  
- **API changes:** helper to compute cache path for (component_id, version_req), with `/`→`-` substitution and blake3/digest support when available.  
- **Tests:** path derivation matches greentic-dev current behavior; handles ids with `/`, `.`, and `@*`.
- **Acceptance:** greentic-dev can drop `cache_base_dir`/`cache_slug_parts` and rely on the helper.

## Candidate PR C (shared resolver crate or distributor-client feature): Component preparation + schema validation
- **Scope:** Centralize: prepare_component call, version gating, describe/schema selection, payload extraction, JSON Schema validation.  
- **API changes:**  
  - Expose a “resolve/prep component” function that accepts a local dir or id + optional component_dir short-name fallback.  
  - Provide a `validate_payload(node_id, component_id, payload)` using selected schema (latest describe).  
  - Optionally expose short-name fallback policy (namespace stripping) as a helper.
- **Tests:**  
  - Version mismatch errors; latest describe selection verified.  
  - Short-name fallback finds `<dir>/foo` for `ns.foo` when `ns.foo` missing.  
  - Schema validation surfaces pointers for invalid payloads.
- **Acceptance:** greentic-dev can drop `component_resolver.rs`’s bespoke logic and use the shared resolver.

## Candidate PR D (optional): Workspace manifest updater
- **Scope:** Provide a small helper to merge/update `.greentic/manifest.json` with component entries and hashes.  
- **API changes:** function to upsert `WorkspaceManifest` entries given coordinate/component_id/version/cache_path.  
- **Tests:** update replaces existing entry for same component; inserts otherwise; preserves unrelated entries.
- **Acceptance:** greentic-dev can delete `update_manifest`.

## Candidate PR E (future): OCI and distributor-internal artifact support
- **Scope:** Implement OCI pull and internal handle resolution so greentic-dev does not hard-reject those ArtifactLocation variants.  
- **Acceptance:** greentic-dev can remove its “not supported yet” branches once upstream handles them.
