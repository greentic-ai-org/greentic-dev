# Local Mocks

`greentic-dev` forwards `pack run` to `greentic-runner-cli`. Any mock behavior comes from the
runner CLI itself, so consult its documentation for supported flags and behavior.

## Ports and endpoints

If your `greentic-runner-cli` build supports mocks, the default ports are:

* **HTTP mocks** – `127.0.0.1:3100` (override with `MOCK_HTTP_PORT`).
* **NATS mock** – `127.0.0.1:4223` (override with `MOCK_NATS_PORT`).
* **Secret vault mock** – `127.0.0.1:8201` (in-memory backend).

Mocks only listen on loopback and are started on demand by the runner CLI.

## Fault injection

Use environment variables or flow metadata to enable specific failure modes:

| Mock           | Env var                    | Behaviour                                 |
| -------------- | -------------------------- | ----------------------------------------- |
| HTTP           | `MOCK_HTTP_FAIL_PATTERN`   | Regex of paths to force 500 responses.    |
| NATS           | `MOCK_NATS_DROP_RATE`      | Fraction (0–1) of messages to drop.       |
| Secret vault   | `MOCK_VAULT_SEAL_AT_START` | When set, mock starts in sealed state.    |

These knobs let you confirm your flow handles partial failures, retries, and transient outages before deploying.
