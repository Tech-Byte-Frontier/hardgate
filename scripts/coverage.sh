#!/usr/bin/env sh
# Produce the LCOV evidence consumed by Hardgate coverage checks.
set -eu

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
mkdir -p coverage
cargo "+$COV_TOOLCHAIN" llvm-cov --all-targets --all-features --locked --branch --include-build-script --lcov --output-path coverage/lcov.info
test -s coverage/lcov.info
