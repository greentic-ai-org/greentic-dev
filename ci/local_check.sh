#!/usr/bin/env bash
set -euo pipefail

export RUSTFLAGS="-D warnings"
export CARGO_TERM_COLOR=always

echo "[local] fmt"
cargo fmt --all --check

echo "[local] clippy"
cargo clippy --all-targets --all-features -- -D warnings

echo "[local] test"
cargo test --all-features --locked

echo "[local] package smoke"
./ci/package_smoke.sh

echo "[local] cargo-dist build (artifacts)"
if ! command -v dist >/dev/null 2>&1; then
  cargo install cargo-dist
fi
DIST_TAG=$(
  cargo metadata --no-deps --format-version=1 | python3 - <<'PY'
import json, sys
data = json.load(sys.stdin)
for pkg in data["packages"]:
    if pkg["name"] == "greentic-dev":
        print(f"v{pkg['version']}")
        break
else:
    raise SystemExit("greentic-dev package not found in metadata")
PY
)
dist build --target x86_64-unknown-linux-gnu --no-local-paths --tag "${DIST_TAG}"

echo "[local] OK"
