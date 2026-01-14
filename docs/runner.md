# Flow Runner

`greentic-dev` does not implement its own runtime runner. All pack execution is delegated to
`greentic-runner-cli` via `greentic-dev pack run`, so flags, output, and artifacts are controlled
by the runner CLI.

For validation, use `greentic-dev flow doctor` (passthrough to `greentic-flow`).
