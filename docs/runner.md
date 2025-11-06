# Flow Runner

The `greentic-dev` runner is built around schema awareness. Every node in a flow is described at runtime so the runner can validate the user-provided configuration before any work is executed.

## Validation vs execution

The runner exposes a dedicated validation mode via:

```bash
greentic-dev run -f examples/flows/min.yaml --validate-only
```

In validation mode the CLI:

1. Loads the flow YAML.
2. Resolves each node’s component schema via the `DescribeRegistry` or a component-provided `describe()` implementation.
3. Runs the JSON Schema validator against the node configuration.
4. Applies any component defaults and records the merged configuration in a transcript.

Because validation skips tool execution it is fast and safe to run in CI.

> Developing from source without installing the binary? Use `cargo run -p greentic-dev -- run …` instead.

Running without `--validate-only` currently performs the same validation steps and then exits. Future work will add actual execution after schema checks pass.

## Discovering schemas

Schemas enter the system in two ways:

* **Registry stubs** – `DescribeRegistry` contains known component schemas and defaults for development.
* **Component `describe()` API** – component binaries or packs expose schemas at runtime. The runner will prefer the dynamically returned schema when available.

During validation the runner combines the discovered schema with the flow YAML to:

* Compile the JSON Schema (Draft 7).
* Report any validation errors with component names and node indexes.
* Capture the schema ID and resolved config in the transcript for downstream tooling.
