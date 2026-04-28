# greentic-dev CLI Guide

`greentic-dev` is a passthrough wrapper over upstream CLIs, plus a launcher wizard.

When invoked as `greentic-dev-dev`, delegated commands resolve to development binaries. For example, `greentic-dev-dev pack ...` runs `greentic-pack-dev`, while `greentic-dev pack ...` runs `greentic-pack`.

## Flow (passthrough to greentic-flow)

- `flow ...` delegates directly to `greentic-flow` (including `--help`).

## Component (passthrough to greentic-component)

- `component ...` delegates directly to `greentic-component` (including `--help`).

## Pack (passthrough to greentic-pack; `pack run` uses greentic-runner-cli)

- `pack ...` delegates to `greentic-pack`.
- `pack run ...` delegates to `greentic-runner-cli`.

## GUI / Secrets / MCP

- `gui ...` delegates to `greentic-gui`.
- `secrets ...` wraps `greentic-secrets` convenience flows.
- `mcp doctor ...` uses the built-in MCP provider metadata inspector.
- `mcp --compose ...` delegates to `greentic-mcp compose ...`.
- other non-`doctor` `mcp` invocations delegate directly to `greentic-mcp`.

## CBOR

- `cbor <file>.cbor` decodes a CBOR payload and prints pretty JSON.

## Coverage

- `greentic-dev coverage`
- `greentic-dev coverage --skip-run`

Behavior:

- ensures `cargo-llvm-cov` and `cargo-nextest` are installed when the run is not skipped
- ensures `llvm-tools-preview` is available through `rustup`
- creates `target/coverage` when missing
- fails with Codex-oriented instructions if `coverage-policy.json` is missing
- writes the report to `target/coverage/coverage.json`
- validates the report against the global floor, per-file defaults, exclusions, and overrides in `coverage-policy.json`
- exits non-zero when setup fails, the coverage run fails, or the policy is violated
- `--skip-run` reuses an existing `target/coverage/coverage.json` file and only evaluates policy compliance

## Install

- `greentic-dev install`
- `greentic-dev install --tenant <TENANT> --token <TOKEN-or-env:VAR>`
- `greentic-dev install --tenant <TENANT> --token <TOKEN-or-env:VAR> --bin-dir <DIR>`
- `greentic-dev install --tenant <TENANT> --token <TOKEN-or-env:VAR> --docs-dir <DIR>`
- `greentic-dev install --tenant <TENANT> --locale <BCP47>`
- `greentic-dev install tools`

Behavior:

- bare `install` prints guidance for bootstrap, customer-approved, and tenant install paths
- `install tools` installs development/bootstrap tools from the canonical Greentic tool catalogue
- `install tools --latest` force-refreshes development/bootstrap tools
- when `--tenant` is present, the command prompts for a hidden token if `--token` is omitted in an interactive terminal
- when `--tenant` is present in a non-interactive context, `--token` is required
- when `--tenant` is present, `greentic-dev` may bootstrap public Greentic tools first
- when a tenant token is available, the command also installs tenant-authorized binaries and docs
- `--locale` selects translated manifest/doc values when available; exact locale is preferred, then language-only fallback (`nl-NL` -> `nl`)
- customer-approved pinned toolchain releases are installed with `gtc install`

Commercial install contract:

- tenant manifests are first resolved from the `greentic-biz/customers-tools` GitHub release tagged `latest`, using the asset `<tenant>.json`
- if no matching GitHub release asset is available, tenant manifests fall back to `oci://ghcr.io/greentic-biz/customers-tools/<tenant>:latest`
- tenant manifests may include expanded tool/doc entries or GitHub-hosted manifest references
- tenant manifests may also use the simple OCI payload shape:
  - tools: `{ id, targets }`
  - docs: `{ url, file_name }`
- commercial binaries and docs must come from GitHub-hosted URLs
- supported target `os` values are `linux`, `macos`, and `windows`
- supported target `arch` values are `x86_64` and `aarch64`
- Linux/macOS archives are expected as `.tar.gz`; Windows archives are expected as `.zip`
- `.tgz` is also accepted for gzip-compressed tarballs

Schema contract:

- tenant manifests should include `$schema` pointing to `tenant-tools.schema.json`
- tool manifests should include `$schema` pointing to `tool.schema.json`
- doc manifests should include `$schema` pointing to `doc.schema.json`
- `schema_version` is currently `"1"`
- `greentic-dev` currently consumes these schema-decorated manifests but does not perform JSON Schema validation before install
- tool/doc manifests may include `i18n` maps keyed by locale such as `nl` or `nl-NL`

Doc manifest notes:

- docs use `source.type = "download"`
- docs include `download_file_name` as part of the manifest contract
- docs use `default_relative_path` for the installed path under the docs root
- `default_relative_path` must remain within the docs directory; path traversal is rejected
- localized doc entries may override `title`, `source.url`, `download_file_name`, and `default_relative_path`
- simple doc entries use `file_name`, which installs directly under the docs root

Default install locations:

- binaries: `$CARGO_HOME/bin` or `~/.cargo/bin`
- docs: `~/.greentic/install/docs`
- state: `~/.greentic/install/state.json`

## Release

- `greentic-dev release generate --release 1.0.5 --from latest`
- `greentic-dev release generate --release 1.0.5 --token env:GHCR_TOKEN`
- `greentic-dev release publish --release 1.0.5 --from latest`
- `greentic-dev release publish --manifest dist/toolchains/gtc-1.0.5.json --tag stable`
- `greentic-dev release publish --release 1.0.5 --from latest --tag rc`
- `greentic-dev release publish --release 1.0.5 --from latest --force`
- `greentic-dev release view --release 1.0.5`
- `greentic-dev release view --tag stable`
- `greentic-dev release latest --token env:GHCR_TOKEN --force`
- `greentic-dev release promote --release 1.0.5 --tag stable`

Behavior:

- `release generate` creates a pinned `dist/toolchains/gtc-<release>.json` manifest from the canonical Greentic tool catalogue
- generated filenames include the source channel: `--from stable` writes `gtc-<release>.json`, `--from dev` writes `gtc-dev-<release>.json`, and other channels write `gtc-<channel>-<release>.json`
- `--from dev` also writes development binary names in the generated manifest, such as `greentic-flow-dev` and `greentic-component-dev`
- `--from` resolves a source manifest/tag for metadata or version constraints; the package/bin list still comes from the canonical catalogue
- if the source manifest does not exist yet, `release generate` bootstraps it at `<repo>:<from>` when GHCR credentials are available
- `release generate --dry-run` shows the generated release manifest and reports the bootstrap it would perform without pushing
- `release publish` generates the pinned manifest and pushes it as `ghcr.io/greenticai/greentic-versions/gtc:<release>`
- `release publish --manifest <FILE>` publishes that local manifest as `gtc:<manifest.version>` without regenerating it
- `release publish --manifest <FILE> --release <release>` publishes the local manifest under the explicit release and uses that value in the pushed manifest
- publishing an existing release tag fails unless `--force` is set
- `release publish --tag <tag>` also moves that tag to the published release manifest
- `release view --release <release>` or `release view --tag <tag>` downloads the selected manifest and prints it as pretty JSON
- `release latest` publishes `gtc:latest` with every catalogue package using `*-dev` bins and `"version": "latest"`
- `release promote --release <release> --tag <tag>` moves a tag to an existing release without regenerating the manifest
- `--token <TOKEN>` and `--token env:<VAR>` authenticate GHCR operations; when omitted, release commands use `GHCR_TOKEN`, then `GITHUB_TOKEN`
- rollback is represented by promoting an older release to the desired tag
- tag names are not restricted; examples include `stable`, `rc`, `demo`, and customer-specific channel names
- release commands do not install local tools

OCI contract:

- toolchain manifests are pushed as OCI artifacts with one JSON layer
- layer media type: `application/vnd.greentic.toolchain.manifest.v1+json`
- default repository: `ghcr.io/greenticai/greentic-versions/gtc`

## Wizard (Launcher-Only)

- `greentic-dev wizard`
- `greentic-dev wizard --dry-run`
- `greentic-dev wizard --answers <FILE>`
- `greentic-dev wizard --answers <FILE> --dry-run`
- `greentic-dev wizard validate --answers <FILE>`
- `greentic-dev wizard apply --answers <FILE>`

Behavior:

- `wizard` is interactive and prompts for launcher action:
  - pack path -> delegates to `greentic-pack wizard`
  - bundle path -> delegates to `greentic-bundle wizard`
- `wizard --answers <FILE>` loads a launcher `AnswerDocument` and executes it directly.
- `wizard --answers <FILE>` also accepts direct `greentic-bundle` / `greentic-pack` AnswerDocuments and wraps them into launcher delegation automatically.
- If the launcher answers include `answers.delegate_answer_document`, the delegated wizard is replayed via its own `wizard apply --answers <FILE>` path instead of opening an inner interactive menu.
- `--dry-run` builds/renders plan without delegated execution.
- `wizard --answers <FILE> --dry-run` builds plan from `AnswerDocument` without delegated execution.
- `validate` builds plan from `AnswerDocument` without delegated execution.
- `apply` builds and executes delegation from `AnswerDocument`.
- `--emit-answers <FILE>` during interactive execution is captured through a delegated answers file and then written back as a launcher AnswerDocument envelope.
- `--emit-answers <FILE>` during dry-run / validate writes the launcher AnswerDocument locally because no delegated wizard executes.
- `wizard run` and `wizard replay` are removed.

Launcher AnswerDocument identity is strict:

- `wizard_id`: `greentic-dev.wizard.launcher.main`
- `schema_id`: `greentic-dev.launcher.main`

Other non-launcher IDs are rejected by `validate` / `apply`.

## Tips

- Missing delegated tools are not auto-installed during passthrough commands. Use `greentic-dev install tools` to bootstrap development tools from the canonical catalogue, or `--latest` to force-refresh.
- Environment overrides:
  - `GREENTIC_DEV_BIN_GREENTIC_FLOW`
  - `GREENTIC_DEV_BIN_GREENTIC_COMPONENT`
  - `GREENTIC_DEV_BIN_GREENTIC_PACK`
  - `GREENTIC_DEV_BIN_GREENTIC_RUNNER_CLI`
  - `GREENTIC_DEV_BIN_GREENTIC_GUI`
  - `GREENTIC_DEV_BIN_GREENTIC_SECRETS`
  - `GREENTIC_DEV_BIN_GREENTIC_MCP`
