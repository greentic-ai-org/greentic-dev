# CI Guidance

## Pull requests

The scaffolded CI workflow executes:

1. `cargo fmt -- --check`
2. `cargo clippy --all-targets -- -D warnings`
3. `cargo test`

Keep PRs green by running these locally before pushing.

## Nightly builds

Optionally schedule nightly jobs that enable feature flags (e.g., `conformance`) or run extended integration tests. Reuse the steps from the PR workflow and add artifact uploads for transcripts or snapshots.

## Secrets

Component CI should not rely on external secrets whenever possible. If a test requires credentials, store them in repository secrets and access them only in trusted workflows.

## Redaction

Ensure logs emitted during CI do not contain sensitive data. When in doubt:

* Mask values before logging.
* Prefer opaque handles (IDs, references) over raw tokens.
* Scrub artifacts prior to uploading them.
