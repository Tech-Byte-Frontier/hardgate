// Offline black-box acceptance gate. Build Hardgate first, then run:
//   node tests/consumer_matrix.mjs
"use strict";

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  CONSUMER_CASES,
  CONSUMER_CASE_IDS,
} from "../scripts/consumer-fixtures.mjs";
import {
  parseExactJson,
  runCase,
  runCheck,
  runConsumerMatrix,
  runMutation,
  runProcess,
  validateGateReport,
  validateMutationReport,
} from "../scripts/consumer-runner.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const runner = path.join(root, "scripts", "check-consumer-matrix.mjs");
const expectedIds = [
  "vite-react-vitest", "next-monorepo-package-local", "jest-fixtures-snapshots", "playwright-suite",
  "package-manager-npm", "package-manager-pnpm", "package-manager-yarn", "package-manager-bun",
  "supabase-roles", "greenfield-strict", "legacy-reference-ratchet",
];

function assertKeys(value, keys, label) { assert.deepEqual(Object.keys(value).sort(), [...keys].sort(), `${label} schema`); }

function emptyReport(overrides = {}) {
  const report = {
    gate_name: "negative-fixture", files_scanned: 1, functions_analyzed: 1, duration_ms: 0, passed: true,
    advisories: [], budget_violations: [], suppression_violations: [], complexity_violations: [], invariant_violations: [],
    clone_violations: [], coverage_violations: [], mutation_violations: [], dead_code_violations: [], orchestration_violations: [],
    summary: {}, top_files: [], ...overrides,
  };
  const fields = ["budget_violations", "suppression_violations", "complexity_violations", "invariant_violations", "clone_violations", "coverage_violations", "mutation_violations", "dead_code_violations", "orchestration_violations"];
  const files = new Set();
  for (const item of [...report.budget_violations, ...report.suppression_violations, ...report.complexity_violations, ...report.invariant_violations, ...report.coverage_violations, ...report.dead_code_violations]) files.add(item.file);
  for (const item of report.clone_violations) [item.file_a, item.file_b].forEach((file) => files.add(file));
  report.summary = { total_errors: fields.reduce((sum, field) => sum + report[field].length, 0), clones: report.clone_violations.length, ast_violations: report.complexity_violations.length, complexity: report.complexity_violations.length, file_budgets: report.budget_violations.length, suppressions: report.suppression_violations.length, architecture: report.invariant_violations.length, coverage: report.coverage_violations.length, mutation: report.mutation_violations.length, dead_code: report.dead_code_violations.length, tool: report.orchestration_violations.length, files_scanned: report.files_scanned, functions_analyzed: report.functions_analyzed, files_with_violations: files.size, passed: report.passed };
  report.top_files = [...files].map((file) => ({ file, violations: 1 }));
  return report;
}

function mutationReport(overrides = {}) {
  return {
    stats: { killed: 1, survived: 0, timeout: 0, compile_error: 0, runner_error: 0, equivalent: 0, unviable: 0, total: 1 }, score: 100, min_score: 85, passed: true, duration_ms: 0,
    results: [{ mutant: { id: 1, file: "src/value.ts", line: 1, column: 1, start_byte: 0, end_byte: 1, original: "+", replacement: "-", description: "operator" }, outcome: "Killed", duration_ms: 0, command: "npm test -- tests/value.test.ts", diagnostic: "", source_restored: true }],
    ...overrides,
  };
}

function orchestrationRecord(step, command, output) {
  return { step, command, exit_code: null, output, recommendation: "restore" };
}

const coverageMissing = orchestrationRecord("coverage-report", "coverage/lcov.info", "Required coverage report was not found.");
const coverageMalformed = orchestrationRecord("coverage-report", "coverage/lcov.info", "Failed to parse required coverage report: malformed lcov.");
const generatedStale = orchestrationRecord("generated-freshness", "node supabase/check-generated.mjs", "Generated artifacts are stale.");
const cloneViolation = { file_a: "src/a.ts", lines_a: [1, 5], file_b: "src/b.ts", lines_b: [2, 6], tokens: 50, lines: 5, fingerprint: "clone", message: "duplicate", recommendation: "extract" };
const legacyViolation = { file: "src/legacy.ts", function_name: "legacy", line_number: 1, end_line: 1, metric: "Parameter Count", actual: 3, limit: 1, breakdown: [], message: "too many parameters", recommendation: "split" };

function writeFakeBinary(dir, mode = "check") {
  const binary = path.join(dir, `fake-${mode}`);
  const gateReports = {
    check: emptyReport(),
    supabase: emptyReport({ passed: false, advisories: ["Classified 2 generated file(s); inventoried without handwritten complexity or clone debt.", "generated-freshness evidence: `node supabase/check-generated.mjs` completed successfully."], files_scanned: 10, functions_analyzed: 2, orchestration_violations: [orchestrationRecord("unsupported-source", "supabase/migrations/001_init.sql", "File is classified as Migration, but no AST engine supports its extension."), orchestrationRecord("unsupported-source", "supabase/seed.sql", "File is classified as Migration, but no AST engine supports its extension.")] }),
    failure: emptyReport({ passed: false, orchestration_violations: [coverageMissing] }),
    "coverage-malformed": emptyReport({ passed: false, orchestration_violations: [coverageMalformed] }),
    "generated-stale": emptyReport({ passed: false, orchestration_violations: [generatedStale] }),
    clone: emptyReport({ passed: false, clone_violations: [cloneViolation] }),
    "legacy-missing": emptyReport({ passed: false, complexity_violations: [legacyViolation] }),
    "legacy-malformed": emptyReport({ passed: false, advisories: ["legacy ratchet: malformed"] , complexity_violations: [legacyViolation] }),
  };
  const mutationReports = {
    default: mutationReport(),
    zero: mutationReport({ stats: { killed: 0, survived: 0, timeout: 0, compile_error: 0, runner_error: 0, equivalent: 0, unviable: 0, total: 0 }, score: 0, passed: false, results: [] }),
    "runner-error": mutationReport({ stats: { killed: 0, survived: 0, timeout: 0, compile_error: 0, runner_error: 1, equivalent: 0, unviable: 0, total: 1 }, score: 0, passed: false, results: [{ mutant: { id: 1, file: "src/value.ts", line: 1, column: 1, start_byte: 0, end_byte: 1, original: "+", replacement: "-", description: "operator" }, outcome: "RunnerError", duration_ms: 0, command: "npm test -- tests/value.test.ts", diagnostic: "runner failed", source_restored: true }] }),
  };
  const script = `#!/usr/bin/env node
const mode = process.env.FAKE_MODE || ${JSON.stringify(mode)};
if (mode === "signal") process.kill(process.pid, "SIGTERM");
if (mode === "timeout") setTimeout(() => {}, 10000);
if (process.argv[2] === "mutate" && mode === "no-target") { process.stderr.write("Error: no source files found for mutation testing: no production source files are eligible\\n"); process.exit(1); }
if (process.argv[2] === "mutate" && mode === "baseline") { process.stderr.write("unmutated baseline failed before mutants\\n"); process.exit(1); }
if (process.argv[2] === "mutate") { const reports = ${JSON.stringify(mutationReports)}; process.stdout.write(JSON.stringify(reports[mode] || reports.default)); process.exit(0); }
const reports = ${JSON.stringify(gateReports)}; const report = reports[mode] || reports.check; process.stdout.write(JSON.stringify(report)); if (mode !== "timeout") process.exit(mode === "failure" || mode === "coverage-malformed" || mode === "generated-stale" || mode === "clone" || mode === "legacy-missing" || mode === "legacy-malformed" || mode === "supabase" ? 1 : 0);
`;
  fs.writeFileSync(binary, script, { mode: 0o755 });
  return binary;
}

function assertProcessFailures(temp, cwd) {
  const missingPath = path.join(temp, "missing");
  assert.throws(() => runConsumerMatrix({ binary: missingPath }), (error) => error.code === "binary-missing" && error.message.includes("explicit --binary does not exist"));
  const directory = path.join(temp, "binary-directory"); fs.mkdirSync(directory);
  assert.throws(() => runConsumerMatrix({ binary: directory }), (error) => error.code === "binary-invalid" && error.message.includes("regular file"));
  const nonExecutable = path.join(temp, "binary-non-executable"); fs.writeFileSync(nonExecutable, "#!/bin/sh\nexit 0\n", { mode: 0o644 });
  assert.throws(() => runConsumerMatrix({ binary: nonExecutable }), (error) => error.code === "binary-invalid" && error.message.includes("executable"));
  const missing = runCheck(path.join(temp, "not-found"), cwd, { expectPass: true, expectedExit: 0 });
  assert.deepEqual({ status: missing.status, reasonCode: missing.reasonCode, exitCode: missing.exitCode }, { status: "fail", reasonCode: "spawn-error", exitCode: null });
  const timeout = runCheck(writeFakeBinary(temp, "timeout"), cwd, { expectPass: true, expectedExit: 0, timeout: 20 });
  assert.deepEqual({ status: timeout.status, reasonCode: timeout.reasonCode, timedOut: timeout.timedOut }, { status: "fail", reasonCode: "timeout", timedOut: true });
  const signal = runCheck(writeFakeBinary(temp, "signal"), cwd, { expectPass: true, expectedExit: 0 });
  assert.deepEqual({ status: signal.status, reasonCode: signal.reasonCode, signal: signal.signal }, { status: "fail", reasonCode: "signal", signal: "SIGTERM" });
  const pass = runCheck(writeFakeBinary(temp), cwd, { expectPass: true, expectedExit: 0 });
  assert.deepEqual({ status: pass.status, reasonCode: pass.reasonCode, exitCode: pass.exitCode }, { status: "pass", reasonCode: "ok", exitCode: 0 });
  const mismatch = runCheck(writeFakeBinary(temp, "failure"), cwd, { expectPass: true, expectedExit: 0 });
  assert.deepEqual({ status: mismatch.status, reasonCode: mismatch.reasonCode, exitCode: mismatch.exitCode }, { status: "fail", reasonCode: "exit-status-mismatch", exitCode: 1 });
}

function fixtureRoot(temp, name) {
  const target = path.join(temp, name);
  fs.cpSync(path.join(root, "tests/fixtures/consumers/package-managers/npm"), target, { recursive: true });
  return target;
}

function assertMutationFailures(temp) {
  const spec = CONSUMER_CASES.find((item) => item.id === "package-manager-npm").mutation;
  const cases = [["baseline", "baseline-failure"], ["no-target", "no-target"], ["zero", "mutation-report-failed"], ["runner-error", "mutation-report-failed"]];
  for (const [mode, reasonCode] of cases) {
    const target = fixtureRoot(temp, `mutation-${mode}`);
    const result = runMutation(writeFakeBinary(temp, mode), target, spec);
    assert.equal(result.status, "fail");
    assert.equal(result.reasonCode, reasonCode, `${mode} must block with its exact reason`);
    fs.rmSync(target, { recursive: true, force: true });
  }
}

function assertReportFailures() {
  for (const text of ["junk{}", "{}{}", "[]", "{bad}"]) assert.throws(() => parseExactJson(text), (error) => error.code === "malformed-report");
  const schema = emptyReport(); schema.extra = true;
  assert.throws(() => validateGateReport(schema), (error) => error.code === "report-schema");
  const status = emptyReport({ passed: false });
  assert.throws(() => validateGateReport(status), (error) => error.code === "report-status");
  const malformedMutation = mutationReport({ score: 0 });
  assert.throws(() => validateMutationReport(malformedMutation), (error) => error.code === "report-status");
  const clone = emptyReport({ passed: false, clone_violations: [cloneViolation] });
  validateGateReport(clone);
}

function assertEvidenceFailures(temp, cwd) {
  const missing = runCheck(writeFakeBinary(temp, "failure"), cwd, { expectPass: false, expectedExit: 1, expectedViolationCount: 1, expectedOrchestration: [coverageMissing] });
  assert.equal(missing.status, "pass", "exact missing coverage evidence must be recognized");
  const malformed = runCheck(writeFakeBinary(temp, "coverage-malformed"), cwd, { expectPass: false, expectedExit: 1, expectedOrchestration: [coverageMissing] });
  assert.equal(malformed.reasonCode, "evidence-mismatch");
  const generated = runCheck(writeFakeBinary(temp, "generated-stale"), cwd, { expectPass: false, expectedExit: 1, expectedOrchestration: [orchestrationRecord("generated-freshness", "node supabase/check-generated.mjs", "Generated artifacts are fresh.")] });
  assert.equal(generated.reasonCode, "evidence-mismatch");
  const clone = runCheck(writeFakeBinary(temp, "clone"), cwd, { expectPass: false, expectedExit: 1, expectedViolationCount: 0 }, true);
  assert.equal(clone.reasonCode, "evidence-mismatch");
  const legacyExpectation = { expectPass: false, expectedExit: 1, expectedViolationCount: 1, legacySummary: { reference: "main", grandfathered: 0, retained: 1 } };
  const legacyMissing = runCheck(writeFakeBinary(temp, "legacy-missing"), cwd, legacyExpectation, true);
  assert.equal(legacyMissing.reasonCode, "evidence-mismatch");
  const legacyMalformed = runCheck(writeFakeBinary(temp, "legacy-malformed"), cwd, legacyExpectation, true);
  assert.equal(legacyMalformed.reasonCode, "evidence-mismatch");
}

function assertTempCleanup(temp) {
  const supabase = CONSUMER_CASES.find((item) => item.id === "supabase-roles");
  const before = new Set(fs.readdirSync(os.tmpdir()).filter((name) => name.startsWith("hardgate-consumer-")));
  const cleaned = runCase(writeFakeBinary(temp, "supabase"), supabase, false);
  assert.equal(cleaned.status, "pass");
  const after = new Set(fs.readdirSync(os.tmpdir()).filter((name) => name.startsWith("hardgate-consumer-")));
  assert.deepEqual(after, before, "consumer fixture temp roots must be cleaned");
}

function runAdversarialChecks() {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-consumer-negative-"));
  const cwd = path.join(temp, "cwd");
  fs.mkdirSync(cwd);
  try {
    assertProcessFailures(temp, cwd);
    assertMutationFailures(temp);
    assertReportFailures();
    assertEvidenceFailures(temp, cwd);
    assertTempCleanup(temp);
  } finally {
    fs.rmSync(temp, { recursive: true, force: true });
  }
}

const result = spawnSync(process.execPath, [runner, "--json"], { cwd: root, encoding: "utf8", env: { ...process.env, HARDGATE_CONSUMER_MATRIX: "ci" } });
if (result.error) assert.fail(`consumer matrix could not spawn: ${result.error.message}`);
assert.equal(result.status, 0, `consumer acceptance matrix is not green (status=${result.status})\n${result.stdout}\n${result.stderr}`);
let report;
assert.doesNotThrow(() => { report = JSON.parse(result.stdout); }, "matrix output must be JSON");
assertKeys(report, ["binary", "cases", "summary"], "matrix");
assertKeys(report.summary, ["pass", "pending", "fail"], "matrix summary");
assert.deepEqual(CONSUMER_CASE_IDS, expectedIds, "consumer case IDs must remain exact");
assert.equal(report.cases.length, expectedIds.length, "matrix must cover every stabilization case exactly once");
assert.deepEqual(report.cases.map((item) => item.id), expectedIds, "matrix case order must remain deterministic");
for (const item of report.cases) {
  assertKeys(item, ["id", "fixture", "status", "requirement", "check", "mutation", "diagnostics"], `case ${item.id}`);
  assert.ok(["pass", "fail"].includes(item.status)); assert.equal(item.diagnostics, null);
  assert.ok(item.check && item.check.status === "pass", `check contract failed for ${item.id}`);
  assertKeys(item.check, ["status", "reasonCode", "diagnostics", "exitCode", "signal", "timedOut", "report"], `check ${item.id}`);
  validateGateReport(item.check.report);
  if (item.mutation) {
    assertKeys(item.mutation, ["status", "reasonCode", "diagnostics", "exitCode", "signal", "timedOut", "report", "commands"], `mutation ${item.id}`);
    assert.equal(item.mutation.status, "pass");
    validateMutationReport(item.mutation.report);
    assert.equal(item.mutation.report.passed, true);
    assert.equal(item.mutation.report.stats.killed, 1);
    assert.equal(item.mutation.report.stats.survived, 0);
  }
}
assert.equal(report.summary.pending, 0, "pending consumer capabilities are blocking");
assert.equal(report.summary.fail, 0, "failed consumer fixtures are blocking");
runAdversarialChecks();
console.log(`consumer_matrix: ${report.summary.pass} fixtures passed; adversarial negatives passed`);
