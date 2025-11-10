#!/usr/bin/env bash
set -euo pipefail

PACKAGE_TARGET_DIR="${CARGO_PACKAGE_TARGET_DIR:-target/package-smoke}"
ORIG_TARGET_DIR="${CARGO_TARGET_DIR-}"
export CARGO_TARGET_DIR="${PACKAGE_TARGET_DIR}"

cargo package --locked --allow-dirty --no-verify

PACKAGE_DIR="${CARGO_TARGET_DIR}/package"
CRATE_TGZ=$(ls "${PACKAGE_DIR}"/*.crate | head -n 1)

if [[ -n "${ORIG_TARGET_DIR-}" ]]; then
  export CARGO_TARGET_DIR="${ORIG_TARGET_DIR}"
else
  unset CARGO_TARGET_DIR || true
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "${TMPDIR}"' EXIT
tar -xzf "${CRATE_TGZ}" -C "${TMPDIR}"
PKGDIR=$(find "${TMPDIR}" -maxdepth 1 -type d -name "*greentic-dev*" | head -n 1)
if [[ -z "${PKGDIR}" ]]; then
  echo "failed to locate unpacked crate directory"
  exit 1
fi
(
  cd "${PKGDIR}"
  cargo check --locked
)
echo "[package_smoke] OK"
