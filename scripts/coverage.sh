#!/usr/bin/env sh
# Produce the LCOV evidence consumed by `hardgate verify`.
set -eu

COV_VERSION="${CARGO_LLVM_COV_VERSION:-0.9.0}"
installed_version=""
if command -v cargo-llvm-cov >/dev/null 2>&1 && output=$(cargo llvm-cov --version 2>/dev/null); then
  installed_version=$(printf '%s\n' "$output" | awk 'NR == 1 { print $2 }')
fi
if [ "$installed_version" != "$COV_VERSION" ]; then
  cargo install cargo-llvm-cov --version "$COV_VERSION" --locked --force
fi
mkdir -p coverage
cargo llvm-cov --all-targets --all-features --locked --lcov --output-path coverage/lcov.info
test -s coverage/lcov.info
