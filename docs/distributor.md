# greentic-dev Distributor Guide

> Practical, slightly opinionated notes for building with greentic-dev now that Distributor integration is in play. Bring your own coffee.

## TL;DR: new remote flows

- `greentic-dev component add component://org/name@^1.2` – resolve via Distributor, download the component artifact, and record it in `.greentic/manifest.json`.
- `greentic-dev pack init --from pack://org/demo-pack@1.0.0` – resolve a pack, cache the bundle, create `./demo-pack/`, drop `bundle.gtpack` there, and unpack it.
- Profile selection: `--profile staging` or `GREENTIC_DISTRIBUTOR_PROFILE=staging`.
- Defaults: intent = `dev`, platform = `wasm32-wasip2`, features = `[]`.

## Configure distributor profiles

Config search order (first existing wins): `GREENTIC_DEV_CONFIG_FILE` → `GREENTIC_CONFIG_FILE` → `GREENTIC_CONFIG` → `$XDG_CONFIG_HOME/greentic-dev/config.toml` → `$HOME/.config/greentic-dev/config.toml` → `$HOME/.greentic/config.toml`. The CLI reports the loaded path and the paths it tried.

Recommended layout:

```toml
[distributor]
default_profile = "default" # override via --profile or GREENTIC_DISTRIBUTOR_PROFILE

[distributor.profiles.default]
base_url = "https://distributor.greentic.cloud"
token = "env:GREENTIC_TOKEN" # read from env var
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

Legacy `[distributor.<name>]` tables continue to work; they are merged with `distributor.profiles` when present.

Runtime selection order:

1. `--profile <name>` flag
2. `GREENTIC_DISTRIBUTOR_PROFILE` env var
3. `distributor.default_profile` (string or inline)
4. `default` profile name

Tokens support `env:VARNAME` indirection; otherwise treated literally.

## Component add – annotated walkthrough

```bash
# Resolve and add a remote component to the current workspace
greentic-dev component add component://greentic/component-llm-openai@^0.3
```

Under the hood:

1) Build `DevResolveRequest`:
   - coordinate = `component://greentic/component-llm-openai@^0.3`
   - intent = `dev`
   - platform = `wasm32-wasip2`
   - features = `[]`
2) POST to `{profile.url}/v1/resolve` with optional `Authorization: Bearer <token>`.
3) If 402 `license_required`, the CLI prints the checkout URL and exits non-zero.
4) On 200:
   - Download via `GET {url}{artifact_download_path}`
   - Cache to `~/.greentic/cache/components/{digest-or-slug}/artifact.wasm`
   - Update `.greentic/manifest.json` with the component entry:

```json
{
  "components": [
    {
      "coordinate": "component://greentic/component-llm-openai@^0.3",
      "entry": {
        "name": "component-llm-openai",
        "version": "0.3.2",
        "file_wasm": "/Users/me/.greentic/cache/components/sha256-abc123/artifact.wasm",
        "hash_blake3": "sha256:abc123"
      }
    }
  ]
}
```

Re-running with the same name replaces the entry.

## Pack init – annotated walkthrough

```bash
greentic-dev pack init --from pack://demo/ultimate-visitor-ai@1.0.0
```

Flow:

1) Resolve with intent = `dev`, platform = `wasm32-wasip2`.
2) Download `bundle.gtpack`, cache to `~/.greentic/cache/packs/{digest-or-slug}/bundle.gtpack`.
3) Create a new directory from the resolved name (slugged), e.g. `./ultimate-visitor-ai/`.
4) Copy `bundle.gtpack` into that directory and unpack it. Existing directories cause an error (no overwrite).

## Cache layout (deterministic)

- Components: `~/.greentic/cache/components/{digest-or-slug}/artifact.wasm`
- Packs: `~/.greentic/cache/packs/{digest-or-slug}/bundle.gtpack`
- `digest-or-slug`:
  - Prefer digest: `sha256-<hex>` from `digest` in the resolve response.
  - Fallback: `{name}-{version}` lowercased, alnum + dashes.

## Error handling cheatsheet

- Network/HTTP errors: bubbled as friendly messages (status + body when available).
- `license_required` (402): prints the message + checkout_url, exits non-zero.
- Wrong kind (e.g., pack resolved when adding component): explicit error.
- Existing directory during `pack init`: fail fast; add `--force` later if needed.

## FAQs

- **Can I change the default platform?** Not yet; it’s hardcoded to `wasm32-wasip2` for now.
- **Where is the manifest updated?** `.greentic/manifest.json` in the current workspace.
- **Does component add touch pack manifests?** No; it only updates the workspace manifest with the resolved component entry.
- **Can I skip the cache?** Not in this version; cache is always populated for reuse.

## Minimal end-to-end example (local mock)

1) Run a local Distributor mock on `http://localhost:7070` that responds to:
   - `POST /v1/resolve` with a component `artifact_download_path`.
   - `GET /v1/artifact/...` with dummy WASM bytes.
2) Configure profile:

```toml
[distributor.default]
url = "http://localhost:7070"
token = ""
```

3) Add component:

```bash
greentic-dev component add component://demo/echo@1.0.0
```

4) Inspect cache:

```bash
ls ~/.greentic/cache/components
cat ~/.greentic/cache/components/*/artifact.wasm
```

5) Inspect workspace manifest:

```bash
cat .greentic/manifest.json
```
