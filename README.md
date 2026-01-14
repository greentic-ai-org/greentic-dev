# Greentic Dev Toolkit

Build and run Greentic automation locally with the same components, flows, and packs you ship to production. `greentic-dev` is a thin, opinionated wrapper around the canonical CLIs (`greentic-component`, `greentic-flow`, `greentic-pack`, `greentic-runner-cli`) so you can scaffold, wire, doctor, and execute with one entrypoint.

Think of it as a developer cockpit:
- **Components**: reusable Wasm units that declare operations and schemas. Everything is a component—business logic, providers, infra hooks.
- **Flows**: YAML graphs that call local or remote components, mix control flow, and compose complex behavior without code sprawl.
- **Packs**: distributable bundles that combine flows + component artifacts for applications, infrastructure, or providers. Packs can depend on remote components, and flows can mix local and remote refs.

The power: you can stitch together local components, pull remote ones, validate against component-provided schemas, and run the whole pack via the runner CLI—all from this CLI.

---

## Quick Start (happy path)

```bash
# 0) Install the toolkit (bundles companion CLIs)
cargo install cargo-binstall
cargo binstall -y greentic-dev

# 1) Scaffold a pack workspace
greentic-dev pack new -- --dir hello-pack dev.local.hello-pack
cd hello-pack

# 2) Scaffold + build + doctor a component
greentic-dev component new --name hello-world --path ./components/hello-world --non-interactive --no-git --no-check
greentic-dev component build --manifest components/hello-world/component.manifest.json
greentic-dev component doctor components/hello-world/target/wasm32-wasip2/release/component_hello_world.wasm \
  --manifest components/hello-world/component.manifest.json

# 3) Wire it into a flow and validate
greentic-dev flow add-step \
  --flow flows/main.ygtc \
  --after start \
  --component dev.local.hello-pack.hello-world \
  --operation handle_message \
  --payload '{}' \
  --routing '[{"out":true}]'
greentic-dev flow doctor flows/main.ygtc --json

# 4) Sync pack manifest, doctor, build, run
greentic-dev pack components -- --in .
greentic-dev pack doctor --pack pack.yaml
greentic-dev pack build -- --in . --gtpack-out dist/hello.gtpack
greentic-dev pack run --pack dist/hello.gtpack --offline --artifacts dist/artifacts
```

That sequence produces a runnable pack that uses your local component, validates the flow against the component’s schema, and executes it locally via the runner CLI.

> Prefer cargo-run? Use `cargo run -p greentic-dev -- <subcommand> …` — semantics are identical.

---

## CLI Overview

This CLI passes through directly to the upstream tools. See the detailed options and examples in [`docs/cli.md`](docs/cli.md).

- `greentic-dev flow …` → `greentic-flow` (doctor, add-step, etc.)
- `greentic-dev component …` → `greentic-component` (new, build, doctor, describe, pack, templates)
- `greentic-dev pack …` → `greentic-pack` (components, update, build, doctor/inspect, run via greentic-runner-cli)
- `greentic-dev gui …` → `greentic-gui` helpers
- `greentic-dev secrets …` → `greentic-secrets` helpers
- `greentic-dev mcp …` → MCP doctor (optional feature)

Links to upstream CLI docs for the full flag sets:
- [`greentic-component/docs/cli.md`](../greentic-component/docs/cli.md)
- [`greentic-flow/docs/cli.md`](../greentic-flow/docs/cli.md)
- [`greentic-pack/docs/cli.md`](../greentic-pack/docs/cli.md)

---

## What You Can Build

- **Applications**: user-facing flows backed by components (LLMs, templating, APIs). Ship as packs, run locally or remotely.
- **Infrastructure**: provisioners, deployers, observability hooks—exposed as components and orchestrated in flows.
- **Providers**: integrate external services (e.g., cloud, messaging). Providers are just components with the right worlds/exports.
- **Hybrid flows**: mix local components you’re iterating on with remote, versioned components pulled from registries.

Everything is validated before execution: flows are checked against component describe schemas; packs are doctored before build; runs can be mocked for fast iteration.

---

## Where to Go Next

- **Full walkthrough**: [`docs/developer-guide.md`](docs/developer-guide.md) (component → flow → pack → run, offline friendly).
- **CLI deep dive**: [`docs/cli.md`](docs/cli.md) (command by command, with links to upstream manuals).
- **Runner & transcripts**: [`docs/runner.md`](docs/runner.md).
- **Scaffolding tips**: [`docs/scaffolder.md`](docs/scaffolder.md).

---

## Requirements

- Rust 1.89+ (repo pins `rust-toolchain.toml`)
- `wasm32-wasip2` target for component builds: `rustup target add wasm32-wasip2`
- Network optional: most flows/components can run offline; remote component pulls require connectivity unless cached.

---

## Contributing

We’re a pass-through CLI by design. If you need new behavior, add it to the upstream tools first (`greentic-flow`, `greentic-component`, `greentic-pack`, `greentic-runner`). Bug reports and PRs welcome! See `.github/workflows/ci.yml` for how we exercise the wrapper against the upstream binaries.
