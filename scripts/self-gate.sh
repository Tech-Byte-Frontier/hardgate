#!/usr/bin/env sh
# Run the complete configured gate plus one real native mutation sample.
# Evidence enablement uses a disposable policy; the project policy is never edited.
set -eu

# Environment hints alone do not cap a compiler or its descendants.
if ! python3 scripts/check-resource-boundary.py >/dev/null 2>&1; then
  exec scripts/with-resource-limits.sh "$0" "$@"
fi
. scripts/resource-worker-env.sh

BINARY="${HARDGATE_BINARY:-target/release/hardgate}"
scripts/coverage.sh
TEMP_POLICY=$(mktemp "$PWD/.hardgate-self-gate.XXXXXX.toml")
cleanup() {
  rm -f "$TEMP_POLICY"
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
' hardgate.toml > "$TEMP_POLICY"
"$BINARY" --config "$TEMP_POLICY" verify --coverage-report coverage/lcov.info --format agent src build.rs

awk '
  /^\[/ { section = $0 }
  section == "[mutation]" && /^enabled = false$/ { $0 = "enabled = true" }
  section == "[coverage]" && /^enabled = false$/ { $0 = "enabled = true" }
  { print }
' hardgate.toml > "$TEMP_POLICY"
# These integration targets exercise the production budget engine without
# recursively starting mutation CLI tests inside an active mutation lease.
# The complete Rust suite runs separately in CI. The bound includes a cold
# stable build with the mutation runner's conservative worker limits.
"$BINARY" --config "$TEMP_POLICY" mutate \
  --scoped src/engines/budgets.rs \
  --test-cmd "cargo test --test static_snapshot --test config_adoption_edges --all-features --locked" \
  --max-mutants 1 \
  --timeout 300 \
  --format agent

rm -f "$TEMP_POLICY"
HARDGATE_BINARY="$BINARY" node scripts/check-consumer-matrix.mjs
