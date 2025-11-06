# {{component_pascal}} Configuration

The component accepts the following YAML structure in flow nodes:

```yaml
component: {{component_name}}
id: my-{{component_kebab}}
inputs:
  message: "Hello from {{component_name}}"
```

## Fields

| Path               | Type   | Required | Default | Description                                 |
|--------------------|--------|----------|---------|---------------------------------------------|
| `component`        | string | ✅        | —       | Must be `{{component_name}}`.               |
| `id`               | string | ➖        | node id | Optional identifier for the node instance.  |
| `inputs.message`   | string | ✅        | —       | Message payload emitted by the component.   |

Defaults come from `schemas/v1/{{component_kebab}}.node.schema.json` and the runner will merge them into transcripts.

## Example

```yaml
nodes:
  - id: hello
    component: {{component_name}}
    inputs:
      message: "Welcome!"
```
