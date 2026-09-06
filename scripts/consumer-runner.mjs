"use strict";

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { CONSUMER_CASES, caseLabel } from "./consumer-fixtures.mjs";
import {
  bounded,
  fail,
  parseExactJson,
  validateGateReport,
} from "./consumer-schema.mjs";
import {
  failureResult,
  processFailure,
  resolveBinary,
  runProcess,
} from "./consumer-process.mjs";
import { initializeFixture } from "./consumer-init.mjs";

export {
  ConsumerMatrixError,
  parseExactJson,
  validateGateReport,
} from "./consumer-schema.mjs";
export { resolveBinary, runProcess } from "./consumer-process.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const FIXTURE_ROOT = path.join(ROOT, "tests", "fixtures", "consumers");

function earlyCheckFailure(processError, result) {
  if (!processError) return null;
  const immediate = ["spawn-error", "signal", "timeout", "no-exit-status"];
  return immediate.includes(processError[0]) ? failureResult(processError[0], processError[1], result) : null;
}

export function runCheck(binary, root, expectation, diff = false) {
  const args = ["check", "--format", "json"];
  if (diff) args.push("--diff");
  const result = runProcess({ binary, args, cwd: root, timeout: expectation.timeout ?? 30_000 });
  const expectedExit = expectation.expectedExit ?? (expectation.expectPass ? 0 : 1);
  const processError = processFailure(result, expectedExit, "check");
  const earlyFailure = earlyCheckFailure(processError, result);
  if (earlyFailure) return earlyFailure;
  let report;
  try {
    report = validateGateReport(parseExactJson(result.stdout, "check"));
  } catch (error) {
    return failureResult(error.code ?? "malformed-report", error.message, result);
  }
  if (processError) return failureResult(processError[0], processError[1], result);
  if (!checkStatusMatches(report, result, expectation, expectedExit)) return failureResult("report-status-mismatch", "check process and report verdict disagree with the expected status", result);
  const failures = checkEvidence(report, expectation);
  if (failures.length) return failureResult("evidence-mismatch", failures.join("; "), result);
  return { status: "pass", reasonCode: "ok", diagnostics: "", exitCode: result.status, signal: null, timedOut: false, report };
}

function checkStatusMatches(report, result, expectation, expectedExit) {
  const passed = result.status === 0 && report.passed === true;
  return report.exit_code === result.status && passed === expectation.expectPass && report.passed === (expectedExit === 0);
}

function checkCountEvidence(report, expectation) {
  const failures = [];
  const counts = [
    ["expectedViolationCount", report.summary.total_errors, "violations"],
    ["minFiles", report.files_scanned, "inventoried files"],
    ["minFunctions", report.functions_analyzed, "parsed functions"],
  ];
  for (const [key, actual, label] of counts) {
    const expected = expectation[key];
    const invalid = key.startsWith("min") ? expected !== undefined && actual < expected : expected !== undefined && actual !== expected;
    if (invalid) failures.push(key.startsWith("min") ? `expected at least ${expected} ${label}, got ${actual}` : `expected ${expected} ${label}, got ${actual}`);
  }
  return failures;
}

function checkOrchestrationEvidence(report, expectation) {
  return (expectation.expectedOrchestration ?? []).flatMap((expected) => {
    const found = report.orchestration_violations.some((item) => item.step === expected.step && item.command === expected.command && item.output === expected.output);
    return found ? [] : [`missing exact orchestration evidence ${expected.step} ${expected.command}`];
  });
}

function checkAdvisoryEvidence(report, expectation) {
  return (expectation.expectedAdvisories ?? []).flatMap((expected) => report.advisories.includes(expected) ? [] : [`missing exact advisory ${expected}`]);
}

function checkComplexityEvidence(report, expectation) {
  return (expectation.expectedComplexity ?? []).flatMap((expected) => {
    const found = report.complexity_violations.some((item) => Object.entries(expected).every(([key, value]) => item[key] === value));
    return found ? [] : [`missing exact complexity evidence for ${expected.file}`];
  });
}

function checkEvidence(report, expectation) {
  const failures = [
    ...checkCountEvidence(report, expectation),
    ...checkOrchestrationEvidence(report, expectation),
    ...checkAdvisoryEvidence(report, expectation),
    ...checkComplexityEvidence(report, expectation),
  ];
  if (expectation.legacySummary) checkLegacySummary(report, expectation.legacySummary, failures);
  return failures;
}

function checkLegacySummary(report, expected, failures) {
  const matching = report.advisories.filter((item) => item.startsWith("legacy ratchet: reference=`"));
  if (matching.length !== 1) { failures.push("legacy ratchet must emit exactly one summary advisory"); return; }
  const value = matching[0].match(/^legacy ratchet: reference=`([^`]+)` merge-base=`([0-9a-f]{40}|[0-9a-f]{64})` grandfathered=(\d+) retained=(\d+)(?:; verdict covers new or worsened blocking static findings in the selected current scope, not a debt-free repository\. Enabled current evidence is still required\.)?$/);
  if (!value || value[1] !== expected.reference || Number(value[3]) !== expected.grandfathered || Number(value[4]) !== expected.retained) failures.push("legacy ratchet summary is malformed or inconsistent");
}

function copyFixture(testCase) {
  const source = path.join(FIXTURE_ROOT, testCase.fixture);
  try {
    if (!fs.statSync(source).isDirectory()) fail("fixture-missing", `fixture is missing: ${testCase.fixture}`);
  } catch (error) {
    if (error instanceof Error && error.code === "fixture-missing") throw error;
    fail("fixture-missing", `fixture is missing: ${testCase.fixture}`);
  }
  const target = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-consumer-"));
  try {
    fs.cpSync(source, target, { recursive: true });
  } catch (error) {
    fs.rmSync(target, { recursive: true, force: true });
    fail("fixture-copy", `could not copy fixture ${testCase.fixture}: ${error.message}`);
  }
  return target;
}


function git(cwd, args) { execFileSync("git", args, { cwd, stdio: "ignore" }); }

function prepareLegacyReference(root) {
  for (const args of [["init", "-q"], ["config", "user.email", "hardgate@example.invalid"], ["config", "user.name", "Hardgate Consumer Fixture"], ["config", "commit.gpgsign", "false"], ["add", "-A"], ["commit", "-qm", "legacy baseline"], ["branch", "-M", "main"], ["switch", "-q", "-c", "consumer-change"]]) git(root, args);
  const source = path.join(root, "src", "legacy.ts");
  fs.writeFileSync(source, fs.readFileSync(source, "utf8").replace("legacy(first: string, second: string)", "legacy(first: string, second: string, third: string)"));
}

function prepareCase(binary, testCase) {
  const root = copyFixture(testCase);
  try {
    if (testCase.initialize) initializeFixture(binary, root, testCase.initialize);
    if (testCase.legacy) prepareLegacyReference(root);
    return root;
  } catch (error) {
    try {
      fs.rmSync(root, { recursive: true, force: true });
    } catch (cleanupError) {
      if (error instanceof Error) error.message = `${error.message}; fixture temp cleanup failed: ${cleanupError.message}`;
    }
    throw error;
  }
}

function cleanupCaseRoot(root, outcome) {
  if (!root) return outcome;
  try {
    fs.rmSync(root, { recursive: true, force: true });
    return outcome;
  } catch (error) {
    const previous = outcome.diagnostics || outcome.check?.diagnostics || "";
    const detail = `fixture temp cleanup failed: ${error.message}`;
    return { ...outcome, status: "fail", diagnostics: bounded(previous ? `${previous}; ${detail}` : detail) };
  }
}

export function runCase(binary, testCase, keepTemp = false) {
  let root;
  let outcome;
  try {
    root = prepareCase(binary, testCase);
    const check = runCheck(binary, root, testCase.check, Boolean(testCase.legacy));
    const status = check.status;
    outcome = { id: testCase.id, fixture: testCase.fixture, status, requirement: testCase.check?.requirement ?? null, check, diagnostics: null };
  } catch (error) {
    outcome = { id: testCase.id, fixture: testCase.fixture, status: "fail", requirement: testCase.check?.requirement ?? null, check: null, diagnostics: bounded(error.message) };
  }
  return keepTemp ? outcome : cleanupCaseRoot(root, outcome);
}

export function runConsumerMatrix(options = {}) {
  const binary = resolveBinary(options);
  const ids = options.caseIds ?? [];
  const unknown = ids.filter((id) => !CONSUMER_CASES.some((testCase) => testCase.id === id));
  if (unknown.length) fail("case-missing", `unknown consumer case id: ${unknown.join(", ")}`);
  const selected = ids.length ? CONSUMER_CASES.filter((testCase) => ids.includes(testCase.id)) : CONSUMER_CASES;
  if (!selected.length) fail("case-missing", "no consumer fixtures matched --case");
  const cases = selected.map((testCase) => runCase(binary, testCase, Boolean(options.keepTemp)));
  const summary = cases.reduce((counts, item) => { counts[item.status] += 1; return counts; }, { pass: 0, pending: 0, fail: 0 });
  return { binary, cases, summary };
}

export function renderHuman(report) {
  for (const result of report.cases) {
    const detail = result.diagnostics || result.check?.diagnostics || "";
    console.log(`${result.status.toUpperCase().padEnd(7)} ${caseLabel(result)}${detail ? ` — ${bounded(detail)}` : ""}`);
  }
  console.log(`consumer matrix: ${report.summary.pass} pass, ${report.summary.pending} pending, ${report.summary.fail} fail`);
}
