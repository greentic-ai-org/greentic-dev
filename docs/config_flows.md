# Config Flows and `flow add-step`

This CLI lets you graft a component’s config flow into an existing pack flow without hand‑editing YAML.

## TL;DR
- Components may ship `flows/default.ygtc` and/or `flows/custom.ygtc`.
- These flows end by emitting a JSON object `{ node_id, node }`.
- `greentic-dev flow add-step <flow-id> --coordinate <component>` runs that flow and splices the returned node into `flows/<flow-id>.ygtc`.

## Quickstart
```bash
# Use a local bundle with config flows
greentic-dev flow add-step onboarding --coordinate ./component-bundle

# Force guided vs default
greentic-dev flow add-step onboarding \
  --coordinate store://meeza/component-qa-process@^0.3 \
  --mode custom

# Patch routing after an existing node
greentic-dev flow add-step onboarding \
  --coordinate ./component-bundle \
  --after start
```

## How it works
1) The CLI locates `flows/<flow-id>.ygtc` in the current workspace.
2) It fetches the component bundle (local path or via Distributor profile/cache).
3) It picks `flows/default.ygtc` (or `custom.ygtc` if requested).
4) It reads the config-flow’s final payload or template and expects JSON with:
   ```json
   { "node_id": "qa_step", "node": { "qa": { "component": "..." }, "routing": [...] } }
   ```
5) It inserts `node` under `nodes[node_id]` in the target flow.
6) If `--after foo` is supplied, it appends a routing edge from `foo` to `node_id`.

## Authoring config flows (component side)
- Place them in `flows/default.ygtc` and/or `flows/custom.ygtc` inside the bundle.
- End the flow with a node that yields `{ node_id, node }` (either via `payload:` or a `template` string that renders to that JSON).
- Keep routing inside `node` as you want it wired; the CLI only appends an edge from `--after` when provided.

## Current limits
- The CLI does not run the flow engine; it reads the final payload/template directly. Keep your config flows simple and deterministic.
- No interactive prompts are shown when both default/custom exist; `--mode` selects explicitly and default is preferred.
- Formatting of the updated flow is best-effort YAML.
