#!/usr/bin/env sh
# Produce the LCOV evidence consumed by Hardgate coverage checks.
set -eu

# Environment hints alone do not cap a compiler or its descendants.
if ! node scripts/check-resource-boundary.mjs >/dev/null 2>&1; then
  exec scripts/with-resource-limits.sh "$0" "$@"
fi
. scripts/resource-worker-env.sh

COV_VERSION="${CARGO_LLVM_COV_VERSION:-0.9.0}"
COV_TOOLCHAIN="${RUST_COVERAGE_TOOLCHAIN:-nightly-2026-09-04}"
installed_version=""
if command -v cargo-llvm-cov >/dev/null 2>&1 && output=$(cargo "+$COV_TOOLCHAIN" llvm-cov --version 2>/dev/null); then
  installed_version=$(printf '%s\n' "$output" | awk 'NR == 1 { print $2 }')
fi
if [ "$installed_version" != "$COV_VERSION" ]; then
  if [ "${HARDGATE_REQUIRE_PREINSTALLED_CARGO_TOOLS:-0}" = 1 ]; then
    echo "hardgate: expected preinstalled cargo-llvm-cov $COV_VERSION, found ${installed_version:-none}" >&2
    exit 1
  fi
  cargo install cargo-llvm-cov --version "=$COV_VERSION" --locked --force
fi
BINARY="${HARDGATE_BINARY:-target/release/hardgate}"
"$BINARY" evidence cargo-llvm-cov --toolchain "$COV_TOOLCHAIN" -- --all-features
test -s .hardgate/evidence/coverage.lcov
test -s .hardgate/evidence/coverage.lcov.hardgate.json
