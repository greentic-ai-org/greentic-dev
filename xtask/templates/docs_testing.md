# Testing {{component_pascal}}

## Test harness

The scaffolded suite lives under `tests/`. Add new integration-style tests that exercise component entry points using representative inputs and verifying outputs.

## Golden files

When using golden snapshots, set `GOLDEN_ACCEPT=1` to regenerate expected outputs:

```bash
GOLDEN_ACCEPT=1 cargo test
```

Remember to unset the variable (or set `GOLDEN_ACCEPT=0`) before committing and ensure all tests pass without accepting new goldens.

## CI

The generated `.github/workflows/ci.yml` runs `cargo fmt`, `cargo clippy`, and `cargo test`. Update it as needed to incorporate additional checks (e.g., docs, integration suites).
