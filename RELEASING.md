## Releasing greentic-dev

1. Update `Cargo.toml` with the new version and land the change on `master`.
2. Run `ci/local_check.sh` to make sure fmt/clippy/tests/build all pass locally.
3. Tag the commit with the final version (`git tag vX.Y.Z && git push origin vX.Y.Z`).
4. GitHub Actions will run the `Release` workflow for the tag: it builds binaries for all supported targets, uploads them to the GitHub Release so `cargo binstall` can fetch them, and then runs the publish stage for crates.io.

The publish workflow verifies that the tag version matches the crate version, so avoid pushing mismatched tags.
