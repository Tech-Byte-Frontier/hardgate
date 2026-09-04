#!/usr/bin/env sh
# Run the pinned RustSec dependency audit used by CI and release gates.
set -eu

AUDIT_VERSION="${CARGO_AUDIT_VERSION:-0.22.2}"
installed_version=""
if command -v cargo-audit >/dev/null 2>&1 && output=$(cargo audit --version 2>/dev/null); then
  installed_version=$(printf '%s\n' "$output" | awk 'NR == 1 { print $2 }')
fi
if [ "$installed_version" != "$AUDIT_VERSION" ]; then
  if [ "${HARDGATE_REQUIRE_PREINSTALLED_CARGO_TOOLS:-0}" = 1 ]; then
    echo "hardgate: expected preinstalled cargo-audit $AUDIT_VERSION, found ${installed_version:-none}" >&2
    exit 1
  fi
  cargo install cargo-audit --version "=$AUDIT_VERSION" --locked --force
fi
cargo audit
