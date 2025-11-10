## Releasing greentic-dev

1. Update `Cargo.toml` with the new version and land the change on `master`.
2. Run `ci/local_check.sh` to make sure fmt/clippy/tests/package smoke/cargo-dist all pass locally.
3. Tag the commit with the final version (`git tag vX.Y.Z && git push origin vX.Y.Z`).
4. GitHub Actions will run the `Release (cargo-dist)` workflow for the tag, upload the binaries, and then the `Publish to crates.io` workflow will dry-run + publish once the Release is published.

The publish workflow verifies that the tag version matches the crate version, so avoid pushing mismatched tags.
