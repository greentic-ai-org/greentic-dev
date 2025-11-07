#!/usr/bin/env bash
set -euo pipefail

# Usage:
#   LOCAL_CHECK_ONLINE=1 LOCAL_CHECK_STRICT=1 ci/local_check.sh
# Defaults: online, non-strict.

: "${LOCAL_CHECK_ONLINE:=1}"
: "${LOCAL_CHECK_STRICT:=0}"
: "${LOCAL_CHECK_VERBOSE:=0}"
: "${LOCAL_CHECK_INSTALL_HOOK:=0}"

if [[ "${LOCAL_CHECK_VERBOSE}" == "1" ]]; then
  set -x
fi

step() {
  printf "\n▶ %s\n" "$*"
}

need() { command -v "$1" >/dev/null || { echo "[miss] $1"; return 1; }; }

run_or_skip() {
  local desc="$1"
  shift
  if "$@"; then
    return 0
  else
    echo "[skip] ${desc}"
    return 0
  fi
}

ensure_tool() {
  local tool="$1"
  local desc="$2"
  if need "$tool"; then
    return 0
  fi
  if [[ "${LOCAL_CHECK_STRICT}" == "1" ]]; then
    echo "[fail] Missing ${tool} required for ${desc}"
    exit 1
  else
    echo "[skip] ${desc} (missing ${tool})"
    return 1
  fi
}

require_online() {
  local desc="$1"
  if [[ "${LOCAL_CHECK_ONLINE}" != "1" ]]; then
    echo "[skip] ${desc} (offline mode)"
    return 1
  fi
  return 0
}

install_prepush_hook() {
  if [[ ! -d .git ]]; then
    return
  fi
  local hook=".git/hooks/pre-push"
  if [[ -e "${hook}" ]]; then
    return
  fi
  cat >"${hook}" <<'HOOK'
#!/usr/bin/env bash
ci/local_check.sh "$@"
HOOK
  chmod +x "${hook}"
  echo "[info] Installed pre-push hook to run ci/local_check.sh"
}

if [[ "${LOCAL_CHECK_INSTALL_HOOK}" == "1" ]]; then
  install_prepush_hook
fi

WORKSPACE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${WORKSPACE_ROOT}"

step "Tool versions"
if ensure_tool rustc "rustc version"; then rustc --version; fi
if ensure_tool cargo "cargo version"; then cargo --version; fi
if ensure_tool python3 "python3 version"; then python3 --version; fi

step "cargo fmt --check"
if ensure_tool cargo "cargo fmt"; then
  cargo fmt --all -- --check
fi

step "Check greentic-types compat shim"
if [[ -f scripts/check_types_compat.py ]] && ensure_tool python3 "compat shim check"; then
  python3 scripts/check_types_compat.py
fi

step "cargo clippy (all targets/all features)"
if ensure_tool cargo "cargo clippy"; then
  cargo clippy --all-targets --all-features -- -D warnings
fi

features_matrix=("" "conformance")
for feat in "${features_matrix[@]}"; do
  label="${feat:-default}"
  step "cargo build (${label})"
  if [[ -z "${feat}" ]]; then
    cargo build --workspace --locked
  else
    cargo build --workspace --locked --features "${feat}"
  fi

  step "cargo test (${label})"
  if [[ -z "${feat}" ]]; then
    cargo test --workspace --locked --all-targets
  else
    cargo test --workspace --locked --all-targets --features "${feat}"
  fi
done

INSTALL_ROOT="target/local-install"
CLI_BIN="${INSTALL_ROOT}/bin/greentic-dev"

step "Install greentic-dev locally (${INSTALL_ROOT})"
if require_online "cargo install greentic-dev" && ensure_tool cargo "cargo install greentic-dev"; then
  cargo install --path . --force --root "${INSTALL_ROOT}"
else
  run_or_skip "cargo install greentic-dev" false
fi

if [[ -x "${CLI_BIN}" ]]; then
  step "CLI smoke tests"
  "${CLI_BIN}" --help >/dev/null
  "${CLI_BIN}" flow --help >/dev/null
  "${CLI_BIN}" component --help >/dev/null
else
  run_or_skip "CLI smoke tests (missing ${CLI_BIN})" false
fi

FLOW_SAMPLE="examples/flows/min.ygtc"
if [[ -x "${CLI_BIN}" && -f "${FLOW_SAMPLE}" ]]; then
  step "Flow validate smoke (${FLOW_SAMPLE})"
  "${CLI_BIN}" flow validate -f "${FLOW_SAMPLE}" --json >/dev/null
else
  run_or_skip "Flow validate smoke" false
fi

if [[ -x "${CLI_BIN}" ]]; then
  step "Pack build + determinism check"
  mkdir -p dist
  PACK_OUT="dist/local-check.gtpack"
  "${CLI_BIN}" pack build -f "${FLOW_SAMPLE}" -o "${PACK_OUT}" --component-dir fixtures/components
  TEMP_DIR="$(mktemp -d)"
  "${CLI_BIN}" pack build -f "${FLOW_SAMPLE}" -o "${TEMP_DIR}/local-check.gtpack" --component-dir fixtures/components
  cmp "${PACK_OUT}" "${TEMP_DIR}/local-check.gtpack"
  rm -rf "${TEMP_DIR}"
else
  run_or_skip "Pack build + determinism check" false
fi

step "Validate all example flows (nightly parity)"
example_globs=(examples/flows/*.ygtc examples/flows/*.yaml)
found_any=0
if [[ -x "${CLI_BIN}" ]]; then
  for glob in "${example_globs[@]}"; do
    for file in ${glob}; do
      [[ -f "${file}" ]] || continue
      found_any=1
      step "Flow validate ${file}"
      "${CLI_BIN}" flow validate -f "${file}" --json >/dev/null
    done
  done
fi
if [[ "${found_any}" -eq 0 ]]; then
  run_or_skip "Example flow validation (no files found)" false
fi

DOCS_SCRIPT="scripts/build_pages.py"
if [[ -f "${DOCS_SCRIPT}" ]]; then
  step "Docs build (cargo doc + build_pages.py)"
  if ensure_tool cargo "cargo doc" && ensure_tool python3 "build_pages.py"; then
    cargo doc --workspace --no-deps
    python3 "${DOCS_SCRIPT}"
  fi
else
  run_or_skip "Docs build (scripts/build_pages.py missing)" false
fi

if require_online "cargo publish --dry-run"; then
  step "cargo publish --dry-run (greentic-dev)"
  if ensure_tool cargo "cargo publish --dry-run"; then
    cargo publish --locked --package greentic-dev --dry-run
  fi
fi

step "Local checks completed"
