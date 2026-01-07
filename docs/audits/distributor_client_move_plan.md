# Plan: Move Transport/Resolve Logic into greentic-distributor-client

## Scope (transport-focused)
- Coordinate parsing for fetch (id + optional `@<semver>`, default `*`).
- Offline/stub hooks to bypass network for dev flows.
- Artifact resolution/download: http(s), file://, local paths; OCI/public GHCR pulls with digest enforcement.
- Cache layout helpers (base dir + slug policy) and download/write helpers.
- Clear errors for unsupported artifact kinds (OCI auth/private, distributor-internal) until implemented.

## Candidate PRs

### PR A: Resolve + Download + Cache Helper
- **API:** `resolve_component(coord: &str, options: ResolveOptions) -> ResolveResult { cache_path, metadata }`
  - Options: offline/stub, pack_id, tenant/env/profile, intent (Dev/Runtime), request timeout.
  - Accepts coordinates without `@` → `*` semver.
- **Behavior:** perform distributor resolve → download artifact (http/file/OCI when supported) → write to cache with slug policy → return cache path + digest/signature info.
- **Tests:** offline without stub fails; offline with stub JSON succeeds; coordinate without `@` uses `*`; http(s) and file:// fetch; clear error for unsupported OCI/internal until PR E.
- **Acceptance:** greentic-dev drops `run_component_add` resolve/download/cache logic and calls this helper.

### PR B: Cache Layout & Slug Policy
- **API:** `cache_path(component_id, version_req, base_dir_opt) -> PathBuf`
  - Default base dir `.greentic/components` (workspace-relative or configurable).
  - Slugging `/` → `-`, include version; optionally digest-based slug variant.
- **Tests:** path derivation matches current greentic-dev behavior; handles ids with `/`, `.`, `@*`.
- **Acceptance:** greentic-dev removes `cache_base_dir`/`cache_slug_parts` duplication.

### PR C: Stub/Offline Support
- **API:** Allow passing a stubbed `ResolveComponentResponse` or a minimal `{ artifact_path, digest }`.
- **Tests:** stub path works; malformed stub errors clearly.
- **Acceptance:** greentic-dev can drop env-var parsing and delegate stub/offline to options.

### PR D: OCI/Public Pull (optional follow-up)
- **Scope:** Add OCI pull support (public, digest/tag) with explicit auth gaps.
- **Tests:** public OCI fetch; error when auth required.
- **Acceptance:** greentic-dev can remove “OCI not supported” branches once upstream handles it.

## Questions / Defaults (recommended)
- Cache layout: keep `.greentic/components/<slug>` with `/`→`-`, include version; allow digest slug option but keep current default for backward compatibility.
- Stubs/offline: prefer explicit options (caller passes stub/offline) rather than env-only; greentic-dev can wrap envs if desired.
- Minimum artifact support: http/file + public OCI (no auth) is sufficient initial target; keep errors for private/internal until implemented.
- Manifest updates: distributor-client should return cache path + metadata; manifest write/upsert should live in greentic-component or a shared helper (see component_move_plan).
- Coordinate parsing: transport layer handles semver-only `@`; richer tag/digest parsing belongs to the OCI fetcher when implemented.
