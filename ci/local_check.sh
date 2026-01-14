#!/usr/bin/env bash
set -euo pipefail

export CARGO_TERM_COLOR=always
export CARGO_NET_RETRY=10
export CARGO_HTTP_CHECK_REVOKE=false

if [[ -z "${CARGO_TARGET_DIR:-}" ]]; then
  export CARGO_TARGET_DIR="$(pwd)/.target-local"
fi

OFFLINE_FLAG=""
LOCKED_FLAG="--locked"
if [[ "${CARGO_NET_OFFLINE:-false}" == "true" ]]; then
  OFFLINE_FLAG="--offline"
  LOCKED_FLAG=""
fi

echo "[check_local] toolchain:"
rustup --version || true
cargo --version

ensure_bin() {
  local bin="$1"
  if command -v "$bin" >/dev/null 2>&1; then
    return 0
  fi
  if [[ -n "${OFFLINE_FLAG}" ]]; then
    echo "[check_local] missing $bin but offline; please install it manually" >&2
    return 1
  fi
  echo "[check_local] installing $bin via cargo binstall"
  cargo binstall ${LOCKED_FLAG} ${OFFLINE_FLAG} -y "$bin"
}

echo "[check_local] ensuring required binaries (greentic-flow, greentic-component, greentic-pack, greentic-runner-cli)"
ensure_bin greentic-flow
ensure_bin greentic-component
ensure_bin greentic-pack
if command -v greentic-runner-cli >/dev/null 2>&1; then
  :
else
  ensure_bin greentic-runner
fi

if [[ -z "${OFFLINE_FLAG}" ]]; then
  echo "[check_local] fetch (locked)"
  if ! cargo fetch --locked; then
    echo "[check_local] cargo fetch failed (offline?). Continuing with existing cache."
    export CARGO_NET_OFFLINE=true
    OFFLINE_FLAG="--offline"
    LOCKED_FLAG=""
  fi
fi

echo "[check_local] fmt + clippy"
cargo fmt --all -- --check
cargo clippy --all --all-features ${LOCKED_FLAG} ${OFFLINE_FLAG} -- -D warnings

echo "[check_local] build (locked)"
cargo build --workspace --all-features ${LOCKED_FLAG} ${OFFLINE_FLAG}

echo "[check_local] test (locked)"
cargo test --workspace --all-features ${LOCKED_FLAG} ${OFFLINE_FLAG} -- --nocapture

echo "[check_local] OK"
