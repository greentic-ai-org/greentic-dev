# Deployment Flows (Events-Based)

Greentic treats deployment automation as a first-class flow pattern without
creating a new flow kind. Deployment flows are simply `type: events` graphs that
operate on a host-provided `DeploymentPlan` and typically render
infrastructure-as-code (IaC) artifacts.

## Characteristics

- **Flow type**: always `events`. There is no special FlowKind for deployment.
- **Ingress**: same implicit ingress rule as other flows — the first node
  receives the `EventEnvelope`.
- **Plan access**: components import the `greentic:deploy-plan@1.0.0` world to
  fetch the current `DeploymentPlan` as JSON and optionally emit status updates.
- **Components**: any component can implement deployment behaviour by declaring
  the appropriate host capabilities (notably `host.iac`) and importing the
  deploy-plan world.
- **Connectors**: packs map deployment flows the same way they map regular event
  flows (for example, triggered by CI/CD or manual actions) — no new connector
  concept is required.

## Scaffolding Helpers

`greentic-flow` exposes a CLI scaffolder to speed up authoring:

```bash
greentic-flow new flows/deploy_stack.ygtc --kind deployment
```

This command:

1. Writes a minimal two-node template where the first node references an opaque
   deployment component (`deploy.renderer` in the template) and reminds authors
   to use the deploy-plan world.
2. Marks the flow as `type: events`.
3. (Optionally) Reads `manifest.yaml` to determine whether the pack declares
   `kind: deployment`, defaulting `--kind deployment` automatically and
   appending the new flow to the manifest’s `flows:` array with a relative path.

Alias shortcuts:

- `--kind deployment` and `--deployment` create the template above.
- `--kind events` leaves you with a generic events flow.
- `--kind messaging` is unchanged.

## Informational Lints

When a pack manifest declares `kind: deployment`, the scaffolder emits a
non-blocking informational note if you still create a messaging flow. Mixed
packs are allowed; this message is simply a reminder that deployment packs are
expected to focus on events flows.

## Authoring Guidelines

- Keep node kinds/ids opaque: `deploy.renderer`, `plan.emit`, and similar names
  are just strings.
- Reference deployment components by their component ID and profile like any
  other node.
- Let components handle provider specifics: the flow only orchestrates logic and
  routing.
- Capture IaC output paths via component config or pack metadata; the flow itself
  does not embed filesystem layout.

By keeping deployment flows indistinguishable from other events flows, authoring
stays approachable for small LLMs and humans alike, while runtimes retain the
flexibility to route plans through any deployment strategy.
