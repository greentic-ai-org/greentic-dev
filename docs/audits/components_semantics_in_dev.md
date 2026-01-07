# Component Resolution Semantics in greentic-dev

## Resolution Algorithm (as implemented today)
1. **Entry points**  
   - `ComponentResolver::resolve_component/resolve_node` → `load_component` (prep from local dir or direct id).  
   - CLI `component add` → `run_component_add` (resolve/download/cache via distributor or stub).
2. **Target resolution for `load_component`** (`component_resolver.rs::component_target`)  
   - If `--component-dir` is set: look for `<dir>/<id>`; if missing, fall back to short name after `.`, `:`, or `/` (e.g., `ai.greentic.hello-world` → `<dir>/hello-world`); otherwise return `<dir>/<id>` path.  
   - If no `component_dir`, treat input as direct id string.
3. **Preparation and version gating** (`load_component`)  
   - Calls `greentic_component::prepare_component(target)` (local only; no remote fetch).  
   - Rejects if prepared manifest.version does not satisfy the semver requirement (empty → `*` via `parse_version_req`).  
   - Caches prepared components by `(name, version)` (name is the requested id, not the manifest id).
4. **Describe/schema selection** (`to_resolved_component`)  
   - Picks the highest describe.version (`choose_latest_version`) and serializes its schema JSON for validation.  
   - Serializes manifest/capabilities/limits into the resolved record.
5. **Node config extraction/validation**  
   - Extracts payload at `/nodes/{node_id}/{component_key}`; builds a pointer for diagnostics.  
   - Validates against selected JSON Schema using `jsonschema` Draft 7; accumulates errors with pointers.

## `component add` / Distributor Path (`src/component_add.rs`)
1. **Local short-circuit**  
   - If coordinate is a filesystem path, returns it without network calls.
2. **Offline/stub behavior**  
   - Reads `GREENTIC_DEV_OFFLINE`; if true and no stub, errors for non-local coordinates.  
   - `GREENTIC_DEV_RESOLVE_STUB` may point to a real `ResolveComponentResponse` JSON or a minimal stub `{artifact_path, digest}` to bypass network.
3. **Coordinate parsing**  
   - Splits on the last `@`; missing `@` yields version `*` (semver req). No tag/digest semantics; plain semver string.
4. **Distributor resolve**  
   - Builds `ResolveComponentRequest` with tenant/env/profile; `extra.intent` set to `Dev` or `Runtime`.  
   - Uses blocking tokio runtime to call `HttpDistributorClient::resolve_component`.  
   - `pack_id` inferred from local `pack.toml` if present, else `greentic-dev-local`.
5. **Artifact fetch**  
   - Supports `ArtifactLocation::FilePath` where path may be http(s), file://, or plain path; downloads via reqwest for URLs.  
   - `ArtifactLocation::OciReference` and `DistributorInternal` are currently rejected (“not supported yet”).  
6. **Caching layout**  
   - Cache base: `<workspace>/.greentic/components/<slug>` where slug = `slugify(component_id-version)` with `/` replaced by `-`.  
   - Writes `artifact.wasm` only; no manifest/schema caching.
7. **Workspace manifest update**  
   - Writes/updates `.greentic/manifest.json` (`WorkspaceManifest`) with `ComponentEntry` pointing to cached wasm, zeroed hash, optional schema/manifest/world left None.

## Ref Parsing / Fallback Rules
- Version requirement: empty → `*` (any). No special handling for tags/digests; OCI refs are explicitly unsupported in `component_add`.
- Namespace-short-name fallback: when resolving from `component_dir`, will try short name after `.`, `:`, or `/` if fully-qualified path missing.
- Cache key: `(requested name, prepared version)`, not normalized to manifest id.

## Classification
- MUST MOVE to distributor-client (or shared helper):  
  - Stub/offline handling (env vars, stub JSON acceptance).  
  - Resolve/download/cache layout and `WorkspaceManifest` update.  
  - Coordinate parsing rules (`@` semver fallback to `*`).  
  - Cache slugging and base path policy.  
  - Artifact fetch handling of http(s)/file:// and rejection of OCI/internal.
- MAY STAY in greentic-dev:  
  - Interactive/UX logging (println summaries).  
  - Path-exists short-circuit for local coordinates (could also live upstream).
- MUST MOVE to a shared component resolver (greentic-flow/pack):  
  - Namespace short-name fallback when using `component_dir`.  
  - Version gating + describe/schema selection + node payload extraction and schema validation (if distributor-client is the owner of resolution semantics, otherwise to a shared “component-resolver” crate).
- DELETE after migration:  
  - Local JSON Schema compile cache and describe selection could be centralized; duplicates upstream capabilities.
