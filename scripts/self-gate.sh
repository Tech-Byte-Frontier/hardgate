#!/usr/bin/env sh
# Run the complete configured gate plus one real native mutation sample.
# The temporary mutation enablement is restored even when a mutant fails.
set -eu

BINARY="${HARDGATE_BINARY:-target/release/hardgate}"
scripts/coverage.sh
CONFIG_BACKUP=$(mktemp)
cp hardgate.toml "$CONFIG_BACKUP"
cleanup() {
  if [ -n "${CONFIG_BACKUP:-}" ] && [ -f "$CONFIG_BACKUP" ]; then
    cp "$CONFIG_BACKUP" hardgate.toml
    rm -f "$CONFIG_BACKUP"
  fi
}
trap cleanup EXIT
trap 'exit 130' HUP INT TERM

"$BINARY" check --all --dead-code --format agent

# `hardgate.toml` keeps both evidence engines disabled for ordinary local
# checks. Enable coverage first and require its real Rust LCOV report for the
# source/build-script scope; the full check above and consumer matrix below
# cover the separately packaged npm wrapper. Then enable mutation for a
# deterministic production-source sample. `mutate` runs the unmutated baseline
# before generating a non-empty mutant set.
awk '
  /^\[/ { section = $0 }
  section == "[coverage]" && /^enabled = false$/ { $0 = "enabled = true" }
  { print }
' "$CONFIG_BACKUP" > hardgate.toml
"$BINARY" verify --coverage-report coverage/lcov.info --format agent src build.rs

awk '
  /^\[/ { section = $0 }
  section == "[mutation]" && /^enabled = false$/ { $0 = "enabled = true" }
  section == "[coverage]" && /^enabled = false$/ { $0 = "enabled = true" }
  { print }
' "$CONFIG_BACKUP" > hardgate.toml
"$BINARY" mutate \
  --scoped src/engines/budgets.rs \
  --test-cmd "cargo test --all-targets --all-features --locked" \
  --max-mutants 1 \
  --timeout 30 \
  --format agent

cp "$CONFIG_BACKUP" hardgate.toml
HARDGATE_BINARY="$BINARY" node scripts/check-consumer-matrix.mjs
