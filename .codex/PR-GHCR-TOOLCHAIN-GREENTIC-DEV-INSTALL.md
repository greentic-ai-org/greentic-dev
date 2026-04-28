PR-GHCR-TOOLCHAIN-GREENTIC-DEV-INSTALL

Title

refactor(install): use canonical Greentic tool catalogue for bootstrap installs

Summary

Refactor `greentic-dev install tools` / `greentic-dev tools install` so it remains a supported development/bootstrap installer backed by the same canonical package catalogue used by `greentic-dev release generate`.

`gtc install` is the customer-approved pinned release installer. `greentic-dev install tools` is the development/bootstrap path for installing the current Greentic public toolchain list with latest/default `cargo binstall` behavior.

Current Code Reality

The current `greentic-dev` install-related code is split across these paths:

- `src/cli.rs`
  - `Command::Tools(ToolsCommand)`
  - `ToolsCommand::Install(ToolsInstallArgs)`
  - `Command::Install(InstallArgs)`
  - `InstallSubcommand::Tools(ToolsInstallArgs)`
  - `ToolsInstallArgs { latest: bool }`
- `src/main.rs`
  - `Command::Tools(command)` routes `ToolsCommand::Install(args)` to `tools::install(args.latest, &selected_locale)`.
  - `Command::Install(args)` routes `Some(InstallSubcommand::Tools(args))` to `tools::install(args.latest, &install_locale)`.
  - bare `Command::Install(args)` routes to `install::run(args)`.
- `src/cmd/tools.rs`
  - `install(latest, locale)` calls `passthrough::install_all_delegated_tools(latest, locale)`.
- `src/passthrough.rs`
  - `install_all_delegated_tools(latest, locale)` calls `ensure_cargo_binstall()` and then installs `DELEGATED_INSTALL_SPECS`.
  - `DELEGATED_INSTALL_SPECS` currently contains 8 binary install specs:
    - `greentic-component` from crate `greentic-component`
    - `greentic-flow` from crate `greentic-flow`
    - `greentic-pack` from crate `greentic-pack`
    - `greentic-runner` from crate `greentic-runner`
    - `greentic-gui` from crate `greentic-gui`
    - `greentic-secrets` from crate `greentic-secrets`
    - `greentic-mcp` from crate `greentic-mcp`
- `src/install.rs`
  - `run(args: InstallArgs)` currently calls `tools::install(false, &locale)?` before tenant handling.
  - With no tenant, bare `greentic-dev install` currently runs the delegated OSS tool installer and returns.
  - With a tenant, bare `greentic-dev install --tenant ...` installs delegated OSS tools first, then tenant tools/docs.

Important mismatch with the GHCR toolchain design:

- The current repo does not contain `gtc`; only update `greentic-dev` behavior and docs here.
- `greentic-dev install tools` currently installs a smaller hard-coded list than the complete Greentic public toolchain.
- `greentic-dev bundle ...` delegates to `greentic-bundle`, but `DELEGATED_INSTALL_SPECS` does not install `greentic-bundle`.
- Release manifest generation needs the same complete public toolchain list that bootstrap installs use.

Required Behavior

`greentic-dev install tools` remains supported and should be redefined as:

```bash
greentic-dev install tools
```

The development/bootstrap installer based on `greentic-dev`'s canonical Greentic public tool catalogue.

Keep these command surfaces:

```bash
greentic-dev install tools
greentic-dev install tools --latest
greentic-dev tools install
greentic-dev tools install --latest
```

Expected command behavior:

```text
greentic-dev install
  -> no longer acts as the full customer installer

greentic-dev install tools
  -> remains supported
  -> installs all tools from greentic-dev's canonical tool catalogue

greentic-dev install --tenant <tenant>
  -> may call install tools first
  -> then installs tenant artifacts/docs
```

Do not print a deprecation warning.

At command start, print:

```text
Installing Greentic development/bootstrap tools.
For customer-approved pinned releases, use `gtc install`.
```

Why this remains supported:

- tenant install may need public Greentic tools available first
- developer machines may need a bootstrap command
- `greentic-dev` must know the complete toolchain list to generate GHCR manifests
- adding a new public tool means updating `greentic-dev`'s canonical catalogue once

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

- `greentic-dev install tools` installs from `GREENTIC_TOOLCHAIN_PACKAGES`.
- `greentic-dev release generate` uses `GREENTIC_TOOLCHAIN_PACKAGES` to generate/pin release manifests.
- `gtc install` consumes published GHCR manifests only.

Implementation Tasks

1. Add shared catalogue

Create `src/toolchain_catalogue.rs` and export it from `src/lib.rs`.

The catalogue should be the single in-repo list of public Greentic toolchain packages and bins. Remove or replace the narrower `DELEGATED_INSTALL_SPECS` list in `src/passthrough.rs`.

2. Refactor installer to use packages and bins

Update `src/passthrough.rs` so `install_all_delegated_tools(latest, locale)` iterates:

- each package in `GREENTIC_TOOLCHAIN_PACKAGES`
- each bin in `package.bins`

For each bin, run:

```bash
cargo binstall -y --locked <crate> --bin <bin>
```

When `--latest` is passed, keep the current force-refresh behavior by adding:

```bash
--force
```

This means the default command is a bootstrap installer: it installs missing tools and lets `cargo-binstall` decide whether an already-installed binary needs work. If the desired behavior becomes "make this machine current" instead of bootstrap, change the implementation to always pass `--force`.

Keep `ensure_cargo_binstall()` behavior.

3. Update command messaging

In `src/cmd/tools.rs::install` or `src/passthrough.rs::install_all_delegated_tools`, print:

```text
Installing Greentic development/bootstrap tools.
For customer-approved pinned releases, use `gtc install`.
```

Do not present this command as legacy, temporary, or scheduled for removal.

4. Adjust bare install behavior

Update `src/install.rs::run(args: InstallArgs)`:

- `greentic-dev install` with no tenant should not present itself as the full customer installer.
- `greentic-dev install --tenant <tenant>` may keep calling `tools::install(false, &locale)?` first, because tenant install may need public Greentic tools available.
- `greentic-dev install tools` and `greentic-dev tools install` remain the explicit bootstrap installer paths.

Suggested no-tenant message:

```text
Use `greentic-dev install tools` for development/bootstrap tools.
Use `gtc install` for customer-approved pinned releases.
Pass `--tenant` to install tenant artifacts and docs.
```

5. Update CLI help text

Update i18n strings in `i18n/en.json` and translated files as needed:

- `cli.command.tools.about`
- `cli.command.tools.install.about`
- `cli.command.install.tools.about`
- `cli.command.tools.install.latest`

Suggested English text:

- `cli.command.tools.about`: `Install Greentic development/bootstrap tool binaries`
- `cli.command.tools.install.about`: `Install tools from the canonical Greentic tool catalogue`
- `cli.command.install.tools.about`: `Install tools from the canonical Greentic tool catalogue`
- `cli.command.tools.install.latest`: `Force-refresh development/bootstrap tool binaries`

6. Update docs

Update docs to distinguish three roles:

- `gtc install`: customer-approved pinned releases from GHCR manifests
- `greentic-dev install tools`: development/bootstrap install from the canonical catalogue
- `greentic-dev install --tenant`: tenant artifacts/docs install, optionally after bootstrap tools

Candidate docs:

- `README.md`
- `docs/cli.md`
- `docs/developer-guide.md`
- `docs/audits/greentic-dev-commercial-install.md`
- `.codex/repo_overview.md`

7. Update tests

Add or adjust tests for:

- `GREENTIC_TOOLCHAIN_PACKAGES` includes all public toolchain packages listed above.
- `GREENTIC_TOOLCHAIN_PACKAGES` contains no duplicate crate/bin pairs.
- `greentic-dev install tools --help` still exists.
- `greentic-dev tools install --help` still exists.
- `install_all_delegated_tools` generates install commands for every package/bin in `GREENTIC_TOOLCHAIN_PACKAGES`.
- `greentic-bundle` is included in the bootstrap install list.
- bare `greentic-dev install` does not claim to be the customer-approved pinned installer.
- `greentic-dev install --tenant` may bootstrap public tools first and still installs tenant docs/artifacts.

Key Rule

When adding a new Greentic public binary:

1. Add it to `greentic-dev`'s canonical tool catalogue.
2. `greentic-dev install tools` can install it.
3. `greentic-dev release generate` includes it in release manifests.
4. `gtc install` installs it only after it appears in a published GHCR release manifest.

Acceptance Criteria

- `greentic-dev install tools` remains a supported command.
- `greentic-dev tools install` remains a supported command.
- no deprecation warning is printed for these commands.
- the bootstrap installer uses `GREENTIC_TOOLCHAIN_PACKAGES`.
- the catalogue includes the full public Greentic toolchain, including `greentic-dev`, `greentic-operator`, `greentic-bundle`, `greentic-setup`, `greentic-start`, and `greentic-deployer`.
- `greentic-dev release generate` can reuse the same catalogue.
- docs clearly distinguish bootstrap installs from customer-approved pinned `gtc install`.
- `gtc` remains the only consumer-side installer for published pinned GHCR release manifests.
