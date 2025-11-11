#!/usr/bin/env bash
set -euo pipefail

export CARGO_TERM_COLOR=always
export CARGO_NET_RETRY=10
export CARGO_HTTP_CHECK_REVOKE=false

if [[ -z "${CARGO_TARGET_DIR:-}" ]]; then
  export CARGO_TARGET_DIR="$(pwd)/.target-local"
fi

echo "[check_local] toolchain:"
rustup --version || true
cargo --version

echo "[check_local] fetch (locked)"
if ! cargo fetch --locked; then
  echo "[check_local] cargo fetch failed (offline?). Continuing with existing cache."
  export CARGO_NET_OFFLINE=true
fi

echo "[check_local] fmt + clippy"
cargo fmt --all -- --check
cargo clippy --all --all-features --locked -- -D warnings

echo "[check_local] build (locked)"
cargo build --workspace --all-features --locked

echo "[check_local] test (locked)"
cargo test --workspace --all-features --locked -- --nocapture

echo "[check_local] OK"
