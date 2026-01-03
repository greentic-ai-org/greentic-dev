# Developer Guide: End-to-End Hello World Component

This guide walks from a clean workstation all the way to a “hello world” Greentic component that you can validate locally. Follow the steps in order; each section calls out the exact commands you need to run.

---

## 1. Prerequisites

1. **Install Rust (via rustup).**
   ```bash
   curl https://sh.rustup.rs -sSf | sh
   source "$HOME/.cargo/env"
   rustup update stable
   ```

2. **Add the WASI target required by `cargo component`.**
   ```bash
   rustup target add wasm32-wasip2
   ```

3. **Install component tooling.**
   ```bash
   cargo install cargo-component --locked
   ```
   > `cargo-component` performs the WIT-driven build; install it once and keep it updated with `cargo install cargo-component --locked --force`.

4. **(Optional) Install supporting CLI tools.**
   - `cargo install wasm-tools --locked` if you want to inspect component metadata with `wasm-tools component wit`.
   - `cargo install just` if you prefer using the repo’s shorthand tasks.

5. **(Optional) Install `greentic-dev` globally.**
   ```bash
   cargo binstall greentic-dev
   ```
   For the latest commit or local forks:
   ```bash
   cargo install --git https://github.com/greentic-ai/greentic-dev greentic-dev
   # or from the current checkout
   cargo install --path .
   ```
   Installing the CLI lets you run `greentic-dev component …` without prefixing commands with `cargo run -p`.

> For most workflows (including this guide) you only need the installed CLI. Clone the `greentic-dev` repository only if you intend to contribute code or work on the tooling itself.

---

## 2. Build a component, then a pack (end-to-end)

The happy path is entirely CLI-driven:

1) **Scaffold the pack workspace.**
```bash
greentic-dev pack new -- --dir ./hello-pack dev.local.hello-pack
cd hello-pack
```
2) **Scaffold the component inside the pack.**
```bash
greentic-dev component new --name hello-world --path ./components/hello-world --non-interactive --no-git --no-check
```
3) **Build and doctor the component.** (Doctor needs either a colocated manifest or an explicit `--manifest`.)
```bash
greentic-dev component build --manifest components/hello-world/component.manifest.json
greentic-dev component doctor components/hello-world/target/wasm32-wasip2/release/component_hello_world.wasm \
  --manifest components/hello-world/component.manifest.json
```
4) **Ensure the component manifest exposes a config flow.** If you skipped flow generation, add or regenerate `dev_flows.default` (e.g., via `greentic-component flow update`). A minimal config flow template that yields a node called `hello` works well.
5) **Wire the component into the pack flow via config flow.** Target the `hello` flow file so the flow id matches the node you’re adding:
```bash
# Optional: rename starter flow to hello (pack.yaml + flows/hello.ygtc)
mv flows/main.ygtc flows/hello.ygtc
sed -i '' 's/id: main/id: hello/' flows/hello.ygtc
greentic-dev flow add-step hello \
  --manifest components/hello-world/component.manifest.json \
  --coordinate components/hello-world \
  --after start
```
6) **Sync pack.yaml components from the components/ directory.** This uses the underlying `packc components` to add your built component entry into `pack.yaml`.
```bash
greentic-dev pack components -- --in .
```
7) **Validate the flow.**
```bash
greentic-dev flow validate -f flows/hello.ygtc --json
```
8) **Build and run the pack locally (offline).**
```bash
greentic-dev pack build -- --in . --gtpack-out dist/hello.gtpack
greentic-dev pack run --pack dist/hello.gtpack --offline --mocks on --artifacts dist/artifacts
```

That sequence yields a runnable pack that pulls a config-flow-defined node from your component, bundles it, and executes it locally without touching the network.

> If doctor/pack build fails, double-check: (a) the WASM is a component (built with `cargo component`), (b) `component.manifest.json` includes `dev_flows.default`, and (c) the pack’s `pack.yaml` references your component artifact.

---

## Quick reference

```
# one-time setup
rustup target add wasm32-wasip2
cargo install cargo-component --locked

# scaffold + build + doctor
greentic-dev pack new -- --dir ./hello-pack dev.local.hello-pack
cd hello-pack
greentic-dev component new --name hello-world --path ./components/hello-world --non-interactive --no-git --no-check
greentic-dev component build --manifest components/hello-world/component.manifest.json
greentic-dev component doctor components/hello-world/target/wasm32-wasip2/release/component_hello_world.wasm \
  --manifest components/hello-world/component.manifest.json

# pack + flow + run
mv flows/main.ygtc flows/hello.ygtc && sed -i '' 's/id: main/id: hello/' flows/hello.ygtc
greentic-dev flow add-step hello --manifest components/hello-world/component.manifest.json --coordinate components/hello-world --after start
greentic-dev pack components -- --in .
greentic-dev flow validate -f flows/hello.ygtc --json
greentic-dev pack build -- --in . --gtpack-out dist/hello.gtpack
greentic-dev pack run --pack dist/hello.gtpack --offline --mocks on

# optional: register and inspect provider extensions
# (this lives outside the main flow wiring steps)
# greentic-dev pack new-provider --pack manifest.cbor --id dev.local.hello.provider --runtime components/hello-world::greentic_provider@greentic:provider/runtime --manifest providers/dev.local.hello.provider/provider.yaml --kind demo
# greentic-pack providers list dist/hello.gtpack
# greentic-pack providers info dist/hello.gtpack --id dev.local.hello.provider
# greentic-pack providers validate dist/hello.gtpack
```
