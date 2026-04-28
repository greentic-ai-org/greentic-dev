PR-GHCR-TOOLCHAIN-GREENTIC-DEV-RELEASE

Title

feat(release): add GHCR toolchain manifest release commands

Summary

Add `greentic-dev release <command>` as the developer-facing workflow for generating, publishing, and promoting GHCR-backed Greentic toolchain manifests.

This PR only concerns `greentic-dev`. The actual customer installer remains `gtc install` in the `gtc` codebase. `greentic-dev` owns the canonical public tool catalogue and the release-operator workflows that produce and move the manifest tags consumed by `gtc`.

Current Code Reality

There is currently no `release` command in this repository.

Current CLI command enum in `src/cli.rs::Command` contains:

- `Flow(PassthroughArgs)`
- `Pack(PassthroughArgs)`
- `Component(PassthroughArgs)`
- `Bundle(PassthroughArgs)`
- `Runner(PassthroughArgs)`
- `Config(ConfigCommand)`
- `Coverage(CoverageArgs)`
- `Mcp(McpCommand)`
- `Gui(PassthroughArgs)`
- `Secrets(SecretsCommand)`
- `Tools(ToolsCommand)`
- `Install(InstallArgs)`
- `Cbor(CborArgs)`
- `Wizard(Box<WizardCommand>)`

Current command routing in `src/main.rs` has no `Command::Release` match arm.

Current install-related OCI code exists in `src/install.rs` for tenant manifests:

- `RealTenantManifestSource`
- `OciPackFetcher`
- `oci_distribution::Reference`
- `oci_distribution::client::Client`
- `RegistryAuth`
- tenant OCI repo constant `CUSTOMERS_TOOLS_REPO`

This release PR can reuse the repo's existing OCI dependency stack, but should not couple release-manifest logic to tenant install logic.

Final CLI Design

All release operations live under:

```bash
greentic-dev release <command>
```

Supported commands:

```bash
greentic-dev release generate --release 1.0.5 --from dev
greentic-dev release publish --release 1.0.5 --from dev
greentic-dev release publish --release 1.0.5 --from dev --force
greentic-dev release publish --release 1.0.5 --from dev --tag rc
greentic-dev release promote --release 1.0.5 --tag stable
```

Optional flags:

```bash
--repo ghcr.io/greenticai/greentic-versions/gtc
--out dist/toolchains
--dry-run
--force
```

Defaults:

- `--repo`: `ghcr.io/greenticai/greentic-versions/gtc`
- `--out`: `dist/toolchains`
- `--from`: `dev`

Command Behavior

1. Generate pinned manifest

```bash
greentic-dev release generate \
  --release 1.0.5 \
  --from dev
```

Behavior:

- Use `GREENTIC_TOOLCHAIN_PACKAGES` as the complete public toolchain package/bin list.
- `--from dev` resolves the source manifest/tag for metadata or constraints, but the package/bin list comes from `GREENTIC_TOOLCHAIN_PACKAGES`.
- Resolve every package in the catalogue to a pinned crate version.
- Generate a manifest where `version` is the requested release string.
- Write the pinned manifest to `dist/toolchains/gtc-1.0.5.json`.
- Do not push anything.

2. Publish release

```bash
greentic-dev release publish \
  --release 1.0.5 \
  --from dev
```

Behavior:

- Generate the pinned manifest as above.
- Push it to GHCR as `ghcr.io/greenticai/greentic-versions/gtc:1.0.5`.
- Fail if `gtc:1.0.5` already exists unless `--force` is set.

3. Publish and tag

```bash
greentic-dev release publish \
  --release 1.0.5 \
  --from dev \
  --tag rc
```

Behavior:

- Generate the pinned manifest.
- Push it to `gtc:1.0.5`.
- Move `gtc:rc` to the same manifest digest.

4. Promote

```bash
greentic-dev release promote \
  --release 1.0.5 \
  --tag stable
```

Behavior:

- Resolve `ghcr.io/greenticai/greentic-versions/gtc:1.0.5`.
- Move `ghcr.io/greenticai/greentic-versions/gtc:stable` to that same manifest digest.
- Do not rebuild or regenerate.

Rollback is the same command pointed at an older release:

```bash
greentic-dev release promote \
  --release 1.0.4 \
  --tag stable
```

Canonical Package Catalogue

Add a shared catalogue in `greentic-dev`, for example in `src/toolchain_catalogue.rs`:

```rust
pub struct ToolchainPackageSpec {
    pub crate_name: &'static str,
    pub bins: &'static [&'static str],
}

pub const GREENTIC_TOOLCHAIN_PACKAGES: &[ToolchainPackageSpec] = &[
    ToolchainPackageSpec {
        crate_name: "greentic-dev",
        bins: &["greentic-dev"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-operator",
        bins: &["greentic-operator"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-bundle",
        bins: &["greentic-bundle"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-setup",
        bins: &["greentic-setup"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-start",
        bins: &["greentic-start"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-deployer",
        bins: &["greentic-deployer"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-component",
        bins: &["greentic-component"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-flow",
        bins: &["greentic-flow"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-pack",
        bins: &["greentic-pack"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-runner",
        bins: &["greentic-runner"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-gui",
        bins: &["greentic-gui"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-secrets",
        bins: &["greentic-secrets"],
    },
    ToolchainPackageSpec {
        crate_name: "greentic-mcp",
        bins: &["greentic-mcp"],
    },
];
```

Then:

- `greentic-dev install tools` installs from `GREENTIC_TOOLCHAIN_PACKAGES` using latest/default behavior.
- `greentic-dev release generate` uses `GREENTIC_TOOLCHAIN_PACKAGES` to generate and pin release manifests.
- `gtc install` consumes published manifests only.

If `src/toolchain_catalogue.rs` already exists from PR-GHCR-TOOLCHAIN-GREENTIC-DEV-INSTALL, reuse it. Do not define a second catalogue in the release module. This release PR should depend on the install/catalogue PR if they are landed separately.

Manifest Model

Add manifest types in a new module, preferably `src/release_cmd.rs` or `src/toolchain_manifest.rs`.

Suggested types:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolchainManifest {
    pub schema: String,
    pub toolchain: String,
    pub version: String,
    pub channel: Option<String>,
    pub created_at: Option<String>,
    pub packages: Vec<ToolchainPackage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolchainPackage {
    #[serde(rename = "crate")]
    pub crate_name: String,
    pub bins: Vec<String>,
    pub version: String,
}
```

The manifest schema string is:

```text
greentic.toolchain-manifest.v1
```

The toolchain value is:

```text
gtc
```

Generated pinned manifests should contain no package with `"version": "latest"`.

Release Version Resolution

For this PR, implement version resolution with a small trait so tests do not hit the network:

```rust
trait CrateVersionResolver {
    fn resolve_latest(&self, crate_name: &str) -> Result<String>;
}
```

The real implementation may use:

```bash
cargo search <crate> --limit 1
```

or crates.io HTTP metadata if already preferable in the codebase.

Keep manifest-generation logic pure and independently testable.

GHCR/OCI Behavior

Use OCI artifacts in GHCR:

```text
ghcr.io/greenticai/greentic-versions/gtc:dev
ghcr.io/greenticai/greentic-versions/gtc:stable
ghcr.io/greenticai/greentic-versions/gtc:beta
ghcr.io/greenticai/greentic-versions/gtc:rc
ghcr.io/greenticai/greentic-versions/gtc:1.0.5
```

These tags are examples only. Do not restrict tag names; channel tags such as `stable`, `rc`, `demo`, `customer-a`, or any other valid OCI tag may be used.

Toolchain manifests are pushed as OCI artifacts with one JSON layer:

```text
application/vnd.greentic.toolchain.manifest.v1+json
```

Release commands should use an auth model consistent with the existing OCI install code:

- read token from environment where possible, such as `GITHUB_TOKEN` or `GHCR_TOKEN`
- use `RegistryAuth` from `oci_distribution`
- emit a clear error if publishing/promoting requires auth and no token is available

Prefer a dedicated release OCI client wrapper instead of extending `RealTenantManifestSource`, because tenant install and toolchain release have different repositories, schemas, and responsibilities.

Implementation Tasks

1. Add shared catalogue

Create `src/toolchain_catalogue.rs` and export it from `src/lib.rs`.

This catalogue must be used by both:

- `greentic-dev install tools`
- `greentic-dev release generate`

If the install/catalogue PR has already added this module, reuse it instead of redefining it. PR-GHCR-TOOLCHAIN-GREENTIC-DEV-RELEASE should depend on PR-GHCR-TOOLCHAIN-GREENTIC-DEV-INSTALL when they are landed independently.

2. Add CLI types

In `src/cli.rs`:

- add `Command::Release(ReleaseCommand)`
- add `ReleaseCommand` enum with `Generate`, `Publish`, and `Promote`
- add args structs:
  - `ReleaseGenerateArgs`
  - `ReleasePublishArgs`
  - `ReleasePromoteArgs`

Use `--release`, not `--version`.

Suggested shape:

```rust
#[derive(Subcommand, Debug)]
pub enum ReleaseCommand {
    Generate(ReleaseGenerateArgs),
    Publish(ReleasePublishArgs),
    Promote(ReleasePromoteArgs),
}
```

3. Wire command routing

In `src/main.rs`:

- import `ReleaseCommand`
- add a `Command::Release(release)` match arm
- route to a new `release_cmd` module:
  - `release_cmd::generate(args)`
  - `release_cmd::publish(args)`
  - `release_cmd::promote(args)`

4. Add localized help

Update `localized_help_command` in `src/cli.rs`:

- add `("release", "cli.command.release.about")` to the root command list
- add `.mut_subcommand("release", ...)` customization for subcommands and args

Update i18n keys in `i18n/en.json` and translated files as needed:

- `cli.command.release.about`
- `cli.command.release.generate.about`
- `cli.command.release.publish.about`
- `cli.command.release.promote.about`
- `cli.command.release.release`
- `cli.command.release.from`
- `cli.command.release.tag`
- `cli.command.release.repo`
- `cli.command.release.out`
- `cli.command.release.dry_run`
- `cli.command.release.force`

5. Add release module

Create `src/release_cmd.rs` and export it from `src/lib.rs`.

Responsibilities:

- parse and validate `ToolchainManifest`
- resolve source references
- pin latest package versions for every package in `GREENTIC_TOOLCHAIN_PACKAGES`
- write generated manifests
- push new release tags
- move channel tags

6. Add pure helpers

Add testable helpers for:

- building `ghcr.io/greenticai/greentic-versions/gtc:<tag>` references
- converting `--from dev` into a source ref
- converting `--release 1.0.5` into a release ref
- generating manifest package entries from `GREENTIC_TOOLCHAIN_PACKAGES`
- rewriting package versions to pinned versions
- validating that generated manifests have `schema == "greentic.toolchain-manifest.v1"`
- validating that generated manifests have `toolchain == "gtc"`

7. Keep release separate from install execution

Do not call:

- `cmd::tools::install`
- `passthrough::install_all_delegated_tools`
- `install::run`

Release commands produce and move manifests. They should not install tools locally except for any explicit internal tool dependency checks needed to publish OCI artifacts.

8. Update docs

Add a release section to:

- `docs/cli.md`
- `README.md`, if a short CLI overview entry is useful
- `.codex/repo_overview.md`

Document:

- generate pinned manifest
- publish versioned release manifest
- publish plus channel tag
- promote existing release to channel
- rollback by promoting an older release
- the shared catalogue relationship between `install tools` and `release generate`

Tests

Add tests for:

- CLI recognizes `greentic-dev release generate --release 1.0.5 --from dev`.
- CLI recognizes `greentic-dev release publish --release 1.0.5 --from dev`.
- CLI recognizes `greentic-dev release publish --release 1.0.5 --from dev --tag rc`.
- CLI recognizes `greentic-dev release promote --release 1.0.5 --tag stable`.
- pinned manifest parses.
- dev manifest with `"latest"` parses.
- generate includes every package/bin in `GREENTIC_TOOLCHAIN_PACKAGES`.
- generate rewrites all `"latest"` package versions.
- generated manifest output path defaults to `dist/toolchains/gtc-<release>.json`.
- promote does not regenerate a manifest.
- release code does not invoke `greentic-dev install tools` or `install_all_delegated_tools`.

Key Rule

When adding a new Greentic public binary:

1. Add it to `greentic-dev`'s canonical tool catalogue.
2. `greentic-dev install tools` can install it.
3. `greentic-dev release generate` includes it in release manifests.
4. `gtc install` installs it only after it appears in a published GHCR release manifest.

Acceptance Criteria

- `greentic-dev release generate` can create a pinned `gtc-<release>.json` from the canonical catalogue.
- `greentic-dev release publish` can push `gtc:<release>` to GHCR.
- `greentic-dev release publish --tag <tag>` can push the release and move a channel tag.
- `greentic-dev release publish --release <release>` fails if the release tag already exists unless `--force` is set.
- channel tags may be overwritten by `promote` or `publish --tag`.
- `greentic-dev release promote --release <release> --tag <tag>` can move a channel tag without rebuilding.
- rollback is represented by promoting an older release tag.
- release commands are documented under `greentic-dev release`.
- release commands use `--release`, not `--version`.
- release generation uses the same `GREENTIC_TOOLCHAIN_PACKAGES` catalogue as `greentic-dev install tools`.
- release commands are independent from install execution.
