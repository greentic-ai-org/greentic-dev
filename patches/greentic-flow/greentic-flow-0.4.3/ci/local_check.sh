#!/usr/bin/env bash
# Usage:
#   LOCAL_CHECK_ONLINE=1 LOCAL_CHECK_STRICT=1 ci/local_check.sh
# Defaults: offline, non-strict.

set -euo pipefail

LOCAL_CHECK_ONLINE=${LOCAL_CHECK_ONLINE:-0}
LOCAL_CHECK_STRICT=${LOCAL_CHECK_STRICT:-0}
LOCAL_CHECK_VERBOSE=${LOCAL_CHECK_VERBOSE:-0}
LOCAL_CHECK_ALLOW_SKIP=${LOCAL_CHECK_ALLOW_SKIP:-0}
SKIPPED_REQUIRED=0

if [[ "${LOCAL_CHECK_VERBOSE}" == "1" ]]; then
  set -x
fi

need() {
  command -v "$1" >/dev/null 2>&1
}

step() {
  echo ""
  echo "▶ $*"
}

skip_step() {
  local reason=$1
  local required=${2:-0}
  if [[ "${required}" == "1" ]]; then
    SKIPPED_REQUIRED=1
  fi
  if [[ "${LOCAL_CHECK_STRICT}" == "1" ]]; then
    echo "[FAIL] ${reason}"
    exit 1
  else
    echo "[skip] ${reason}"
  fi
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

step "Toolchain versions"
if need rustc; then
  rustc --version
else
  skip_step "rustc not found"
fi
if need cargo; then
  cargo --version
else
  skip_step "cargo not found"
fi

step "cargo fmt --all -- --check"
if need cargo; then
  cargo fmt --all -- --check
else
  skip_step "cargo fmt requires cargo"
fi

step "cargo clippy --all-targets --all-features -D warnings"
if need cargo; then
  cargo clippy --all-targets --all-features -- -D warnings
else
  skip_step "cargo clippy requires cargo"
fi

step "cargo build --workspace --locked"
if need cargo; then
  cargo build --workspace --locked
else
  skip_step "cargo build requires cargo"
fi

step "cargo test --all-features"
if need cargo; then
  cargo test --all-features
else
  skip_step "cargo test requires cargo"
fi

step "ygtc-lint --json smoke test"
if ! need python3; then
  skip_step "python3 required for smoke test" 1
elif ! need cargo && [[ ! -x target/debug/ygtc-lint ]]; then
  skip_step "cargo required to build ygtc-lint" 1
else
  if [[ ! -x target/debug/ygtc-lint ]]; then
    cargo build --quiet --bin ygtc-lint
  fi
  ./target/debug/ygtc-lint --json tests/data/flow_ok.ygtc | python3 -c 'import json,sys; data=json.load(sys.stdin); assert data.get("ok") is True, data'
fi

step "Verify published schema \$id"
if [[ "${LOCAL_CHECK_ONLINE}" != "1" ]]; then
  skip_step "online schema check disabled (set LOCAL_CHECK_ONLINE=1)" 0
elif ! need curl; then
  skip_step "curl required for schema check" 1
elif ! need python3; then
  skip_step "python3 required for schema check" 1
else
  url="https://raw.githubusercontent.com/greentic-ai/greentic-flow/refs/heads/master/schemas/ygtc.flow.schema.json"
  tmp_schema="$(mktemp)"
  if ! curl -sSf "${url}" -o "${tmp_schema}"; then
    skip_step "schema fetch failed (offline?). Skipping schema parity check." 0
  else
  TMP_SCHEMA="${tmp_schema}" python3 - <<'PY'
import json, os, sys
published = json.load(open(os.environ["TMP_SCHEMA"]))
local = json.load(open("schemas/ygtc.flow.schema.json"))
if published.get("$id") != local.get("$id"):
    raise SystemExit(f"Schema $id mismatch: remote={published.get('$id')} local={local.get('$id')}")
PY
  rm -f "${tmp_schema}"
  fi
fi

if [[ "${SKIPPED_REQUIRED}" == "1" && "${LOCAL_CHECK_ALLOW_SKIP}" != "1" ]]; then
  echo ""
  echo "[FAIL] Required CI steps were skipped. Re-run with LOCAL_CHECK_ONLINE=1 and all tools installed, or set LOCAL_CHECK_ALLOW_SKIP=1 to override."
  exit 2
fi

echo ""
echo "✅ local_check completed"
