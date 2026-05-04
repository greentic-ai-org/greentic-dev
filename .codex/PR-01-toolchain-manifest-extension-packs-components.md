# PR-01: Extend Toolchain Manifest With Extension Packs and Components

Repo: `greentic-dev`

## Goal

Extend the human-editable toolchain manifest so install tooling can know which extension packs and components belong to a toolchain release without embedding digests or canonical OCI refs.

## Schema

```json
{
  "schema": "greentic.toolchain-manifest.v1",
  "toolchain": "gtc",
  "version": "1.0.16",
  "channel": "stable",
  "packages": [],
  "extension_packs": [
    {
      "id": "greentic.messaging.webchat-gui",
      "version": "0.5.4"
    }
  ],
  "components": [
    {
      "id": "greentic.components.adaptive-card-renderer",
      "version": "0.5.8"
    }
  ]
}
```

## Implementation

Add manifest structs:

```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtensionPackRef {
    pub id: String,
    pub version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentRef {
    pub id: String,
    pub version: String,
}
```

Extend the manifest struct:

```rust
pub struct ToolchainManifest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension_packs: Option<Vec<ExtensionPackRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<ComponentRef>>,
}
```

Use `Option<Vec<_>>` for backward compatibility with existing manifests.
The new fields must be omitted when absent so existing generated manifests do not churn.

## Generation Behavior

- Existing manifests without `extension_packs` or `components` must still parse.
- Manifests with the new fields must round-trip through `read_manifest_file` / publish input.
- Release generation must include all tracked `GREENTIC_EXTENSION_PACK_PACKAGES` and `GREENTIC_COMPONENT_PACKAGES` entries.
- Generated pack/component refs should resolve their default versions from the highest semver tag currently published on GHCR.
- If a source manifest supplies a non-`latest` version for a tracked pack/component id, release generation should preserve that version.
- `latest_manifest` should include all tracked pack/component refs with version `latest`.
- `--from dev` should generate `*-dev` binary names; `--from rnd` should generate `*-rnd` binary names.

## Rules

- No digests.
- No canonical refs.
- Versions only.
- Manifest stays human-editable.
- `channel` and `version` identify the toolchain/release context, not the resolved artifact digests.

## Output Contract

This PR only exposes the manifest data. It should not generate the release index.
Release-index generation belongs in `gtc`, not `greentic-dev`.
