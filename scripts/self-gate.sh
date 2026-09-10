#!/usr/bin/env sh
# Run the configured gate with coverage and required specialist mutation evidence.
# Explicit report arguments require both fresh evidence engines without changing policy.
set -eu

# Environment hints alone do not cap a compiler or its descendants.
if ! node scripts/check-resource-boundary.mjs >/dev/null 2>&1; then
  exec scripts/with-resource-limits.sh "$0" "$@"
fi
# Fresh private producer builds need headroom below the enforced memory ceiling.
# Debug symbols are unnecessary for test behavior and LLVM coverage mappings.
: "${CARGO_BUILD_JOBS:=1}"
: "${CARGO_PROFILE_DEV_DEBUG:=0}"
: "${CARGO_PROFILE_TEST_DEBUG:=0}"
export CARGO_BUILD_JOBS CARGO_PROFILE_DEV_DEBUG CARGO_PROFILE_TEST_DEBUG
. scripts/resource-worker-env.sh

BINARY="${HARDGATE_BINARY:-target/release/hardgate}"
if [ -z "${HARDGATE_MUTATION_REPORT:-}" ]; then
  # Preserve the original single production budget-engine mutation sample.
  # This semantic replacement removes all measured budget violations; it is
  # explicit sampling, not a repository-wide mutation coverage claim.
  "$BINARY" evidence cargo-mutants -- \
    --file src/engines/budgets.rs \
    --re 'replace check_measured_budgets -> Vec<BudgetViolation> with vec!\[\]$' \
    --timeout 300
  HARDGATE_MUTATION_REPORT=.hardgate/evidence/mutation.json
fi
if [ -z "${HARDGATE_COVERAGE_REPORT:-}" ]; then
  HARDGATE_BINARY="$BINARY" scripts/coverage.sh
  HARDGATE_COVERAGE_REPORT=.hardgate/evidence/coverage.lcov
fi
"$BINARY" check --format agent

# Rust producer evidence covers Rust source/build scripts. The complete check
# above includes repository policy and configured tools. The offline matrix
# below verifies CLI policy/partial/failure contracts; npm archive installation
# and real specialist consumer trials are separate acceptance checks.
sample_status=0
"$BINARY" check --checks policy \
  --mutation-report "$HARDGATE_MUTATION_REPORT" \
  --coverage-report "$HARDGATE_COVERAGE_REPORT" --format agent \
  --report-json .hardgate/evidence/self-gate-sample-check.json src build.rs || sample_status=$?
# A real native sample is useful release evidence, but cannot certify exhaustive
# mutation scope. Require that exact incomplete result and no other gate failures.
test "$sample_status" = 2
node --input-type=module - <<'JS'
import assert from 'node:assert/strict';
import fs from 'node:fs';
const report = JSON.parse(fs.readFileSync('.hardgate/evidence/self-gate-sample-check.json', 'utf8'));
assert.equal(report.status, 'incomplete');
assert.equal(report.accepted, false);
assert.equal(report.summary.analysis_blockers, 1);
assert.equal(report.summary.code_findings, 0);
assert.equal(report.orchestration_violations.length, 1);
assert.equal(report.orchestration_violations[0].step, 'mutation-scope');
assert.match(report.orchestration_violations[0].output, /unnamed mutation evidence is a sample/);
assert.equal(report.execution.engines.find(engine => engine.id === 'coverage').state, 'completed');
assert.equal(report.execution.engines.find(engine => engine.id === 'mutation_report').state, 'incomplete');
console.log('Verified real coverage and native mutation sample; exhaustive mutation acceptance remains incomplete.');
JS
HARDGATE_BINARY="$BINARY" node scripts/check-consumer-matrix.mjs
