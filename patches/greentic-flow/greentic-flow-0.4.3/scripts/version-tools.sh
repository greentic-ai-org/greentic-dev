#!/usr/bin/env bash
set -euo pipefail

# Prints "name version manifest_path" lines for all crates in the workspace (or single crate).
list_crates() {
  cargo metadata --format-version 1 --no-deps \
    | jq -r '.packages[] | "\(.name) \(.version) \(.manifest_path)"'
}

# Given a manifest path, returns the crate relative dir.
crate_dir_from_manifest() {
  local manifest="$1"
  dirname "$manifest"
}

# For single-crate repos (no [workspace]), return only the root package.
is_workspace() {
  if grep -q "^\[workspace\]" Cargo.toml; then
    echo "1"
  else
    echo "0"
  fi
}
