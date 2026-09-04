"use strict";

import { execFileSync } from "node:child_process";
import crypto from "node:crypto";
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
  validateMutationReport,
} from "./consumer-schema.mjs";
import {
  failureResult,
  processFailure,
  resolveBinary,
  runProcess,
} from "./consumer-process.mjs";
import { invocationIdentityFailures } from "./consumer-invocation.mjs";

export {
  ConsumerMatrixError,
  parseExactJson,
  validateGateReport,
  validateMutationReport,
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
  const passed = result.status === 0 && report.passed === true;
  if (passed !== expectation.expectPass || report.passed !== (expectedExit === 0)) return failureResult("report-status-mismatch", `check exit/report status does not match expected pass=${expectation.expectPass}`, result);
  const failures = checkEvidence(report, expectation);
  if (failures.length) return failureResult("evidence-mismatch", failures.join("; "), result);
  return { status: "pass", reasonCode: "ok", diagnostics: "", exitCode: result.status, signal: null, timedOut: false, report };
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
  const value = matching[0].match(/^legacy ratchet: reference=`([^`]+)` merge-base=`([0-9a-f]{40}|[0-9a-f]{64})` grandfathered=(\d+) retained=(\d+)$/);
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

function initializeFixture(binary, root, preset) {
  const result = runProcess({ binary, args: ["init", "--preset", preset], cwd: root });
  const error = processFailure(result, 0, "init");
  if (error) fail(error[0], error[1]);
  const configPath = path.join(root, "hardgate.toml");
  if (!fs.existsSync(configPath) || !fs.statSync(configPath).isFile()) fail("fixture-init", "hardgate init did not write hardgate.toml");
  const config = fs.readFileSync(configPath, "utf8");
  if (!config.includes('preset = "strict-agent"') || !config.includes("strict = true") || config.includes('"tests/**"')) fail("fixture-init", "strict init wrote an unexpected policy");
}

function git(cwd, args) { execFileSync("git", args, { cwd, stdio: "ignore" }); }

function prepareLegacyReference(root) {
  for (const args of [["init", "-q"], ["config", "user.email", "hardgate@example.invalid"], ["config", "user.name", "Hardgate Consumer Fixture"], ["config", "commit.gpgsign", "false"], ["add", "-A"], ["commit", "-qm", "legacy baseline"], ["branch", "-M", "main"], ["switch", "-q", "-c", "consumer-change"]]) git(root, args);
  const source = path.join(root, "src", "legacy.ts");
  fs.writeFileSync(source, fs.readFileSync(source, "utf8").replace("legacy(first: string, second: string)", "legacy(first: string, second: string, third: string)"));
}

function enableMutation(root) {
  const configPath = path.join(root, "hardgate.toml");
  const config = fs.readFileSync(configPath, "utf8");
  const enabled = config.replace(/(\[mutation\]\s*\n\s*enabled\s*=\s*)false/, "$1true");
  if (enabled === config) fail("fixture-config", "fixture config has no disabled mutation section");
  fs.writeFileSync(configPath, enabled);
}

function sourceSnapshot(root, relative) {
  const file = path.join(root, relative);
  try {
    const bytes = fs.readFileSync(file);
    return { file, bytes, hash: crypto.createHash("sha256").update(bytes).digest("hex"), mode: fs.statSync(file).mode & 0o7777 };
  } catch (error) {
    fail("source-missing", `mutation source is missing: ${relative} (${error.message})`);
  }
}

function installHarness(root, spec, snapshot, testSnapshot) {
  const packageRoot = path.resolve(root, spec.packageRoot);
  const workspaceRoot = path.resolve(root, spec.workspaceRoot);
  const packageBin = path.join(packageRoot, "node_modules", ".bin");
  const workspaceBin = path.join(workspaceRoot, "node_modules", ".bin");
  fs.mkdirSync(packageBin, { recursive: true });
  fs.mkdirSync(workspaceBin, { recursive: true });
  const inheritedPath = (process.env.PATH ?? "").split(path.delimiter)
    .filter((entry) => !path.resolve(entry).endsWith(path.join("node_modules", ".bin")));
  const pathValue = (packageBin === workspaceBin ? [] : [workspaceBin]).concat(inheritedPath).join(path.delimiter);
  const expectedPathBins = packageBin === workspaceBin ? [packageBin] : [packageBin, workspaceBin];
  const harness = path.join(root, ".consumer-harness.mjs");
  fs.writeFileSync(harness, `import fs from "node:fs"; import crypto from "node:crypto"; import path from "node:path"; import { evaluateBehavior } from ${JSON.stringify(path.resolve(ROOT, "scripts/consumer-behavior.mjs"))};\nconst hash=p=>crypto.createHash("sha256").update(fs.readFileSync(p)).digest("hex");\nconst source=process.env.CONSUMER_SOURCE; const test=process.env.CONSUMER_TEST; const sourceText=fs.readFileSync(source,"utf8"); const testText=fs.readFileSync(test,"utf8"); const sourceHash=hash(source); const testHash=hash(test); const behavior=evaluateBehavior(sourceText,testText,JSON.parse(process.env.CONSUMER_BEHAVIOR)); const argv=process.argv.slice(2); const expected=JSON.parse(process.env.CONSUMER_ARGV); const executable=fs.realpathSync(process.env.CONSUMER_EXECUTABLE); const pathEntries=(process.env.PATH??"").split(path.delimiter).filter(Boolean).map(entry=>path.resolve(entry)); const pathBins=pathEntries.filter(entry=>entry.endsWith(path.join("node_modules",".bin"))); const expectedPathBins=JSON.parse(process.env.CONSUMER_EXPECTED_PATH_BINS); const record={cwd:process.cwd(), manager:path.basename(executable), managerEnv:process.env.CONSUMER_MANAGER, argv, executable, packageRoot:process.env.CONSUMER_PACKAGE_ROOT, workspaceRoot:process.env.CONSUMER_WORKSPACE_ROOT, path:process.env.PATH??"", pathEntries, pathBins, pathBinsExpected:JSON.stringify(pathBins)===JSON.stringify(expectedPathBins), sourceHash, testHash, sourceMarker:sourceText.includes(process.env.CONSUMER_SOURCE_MARKER), behaviorExpected:JSON.parse(process.env.CONSUMER_BEHAVIOR).expected, behaviorActual:behavior.actual, behaviorPassed:behavior.passed, behaviorReason:behavior.reason, testExists:fs.statSync(test).isFile(), argvExpected:JSON.stringify(argv)===JSON.stringify(expected)}; fs.appendFileSync(process.env.CONSUMER_LOG, JSON.stringify(record)+"\\n"); process.exitCode=record.testExists && testHash===process.env.CONSUMER_TEST_HASH && record.argvExpected && record.pathBinsExpected && record.managerEnv===record.manager && record.behaviorPassed ? 0 : 1;\n`);
  const managerPath = path.join(packageBin, spec.manager);
  fs.writeFileSync(managerPath, `#!/bin/sh\nset -eu\nCONSUMER_EXECUTABLE="$0" CONSUMER_MANAGER="${spec.manager}" exec node "$CONSUMER_HARNESS" "$@"\n`, { mode: 0o755 });
  return {
    log: path.join(root, ".consumer-command-log"),
    env: {
      CONSUMER_HARNESS: harness, CONSUMER_LOG: path.join(root, ".consumer-command-log"),
      CONSUMER_SOURCE: snapshot.file, CONSUMER_TEST: testSnapshot.file,
      CONSUMER_SOURCE_HASH: snapshot.hash, CONSUMER_TEST_HASH: testSnapshot.hash,
      CONSUMER_SOURCE_MARKER: spec.sourceMarker, CONSUMER_ARGV: JSON.stringify(spec.argv),
      CONSUMER_PACKAGE_ROOT: packageRoot, CONSUMER_WORKSPACE_ROOT: workspaceRoot,
      CONSUMER_EXPECTED_PATH_BINS: JSON.stringify(expectedPathBins), CONSUMER_BEHAVIOR: JSON.stringify(spec.behavior), PATH: pathValue,
    },
    packageRoot, workspaceRoot, packageBin, workspaceBin, managerPath,
  };
}

function readCommands(log) {
  if (!fs.existsSync(log)) return [];
  const lines = fs.readFileSync(log, "utf8").trim().split("\n").filter(Boolean);
  try { return lines.map((line) => JSON.parse(line)); } catch (error) { fail("command-log", `consumer command log is malformed: ${error.message}`); }
}

function mutationProcessFailure(result) {
  if (result.status === 1 && /no source files found for mutation testing|no viable AST mutation points/i.test(result.stderr)) return ["no-target", "mutation run found no eligible production target"];
  if (result.status === 1 && /unmutated baseline/i.test(result.stderr)) return ["baseline-failure", "mutation baseline failed before mutants were executed"];
  return processFailure(result, 0, "mutation");
}

function earlyMutationFailure(processError, result, commands) {
  if (!processError) return null;
  const immediate = ["baseline-failure", "no-target", "spawn-error", "signal", "timeout", "no-exit-status"];
  return immediate.includes(processError[0]) ? failureResult(processError[0], processError[1], result, { commands }) : null;
}

function parseMutationReport(result, commands) {
  try {
    return { report: validateMutationReport(parseExactJson(result.stdout, "mutation")), failure: null };
  } catch (error) {
    return { report: null, failure: failureResult(error.code ?? "malformed-report", error.message, result, { commands }) };
  }
}

function mutationSummaryFailure(report, result, commands) {
  const truthful = report.passed && report.stats.killed === 1 && report.stats.survived === 0 && report.stats.total === 1 && report.score === 100;
  return truthful ? null : failureResult("mutation-report-failed", "mutation report did not record one truthful killed mutant", result, { commands });
}

function mutationResultFailures(report, spec) {
  const mutationResult = report.results[0];
  const mutant = mutationResult?.mutant;
  const failures = [];
  if (!mutationResult || mutationResult.outcome !== "Killed") failures.push("representative mutant was not killed");
  if (!mutant || mutant.file !== spec.sourcePath) failures.push(`mutation target must be exactly ${spec.sourcePath}`);
  if (mutationResult?.command !== `${spec.manager} ${spec.argv.join(" ")}`) failures.push("mutation command does not match the resolved selector");
  if (!mutationResult?.source_restored) failures.push("mutation report did not confirm source restoration");
  return failures;
}

function invocationAssertionFailures(command, index, testSnapshot) {
  const failures = [];
  const position = index + 1;
  if (command.testHash !== testSnapshot.hash) failures.push(`invocation ${position} test source changed during mutation`);
  if (command.behaviorPassed !== (index === 0)) failures.push(`invocation ${position} behavior assertion outcome was unexpected`);
  if (!command.testExists || !command.argvExpected) failures.push(`invocation ${position} fixture assertion failed`);
  return failures;
}

function invocationFailures(command, index, context) {
  return [
    ...invocationIdentityFailures(command, index + 1, context.harness, context.spec),
    ...invocationAssertionFailures(command, index, context.testSnapshot),
  ];
}

function mutationEvidenceFailures(report, commands, context) {
  const failures = mutationResultFailures(report, context.spec);
  if (commands.length !== 2) failures.push(`expected exactly two test invocations, got ${commands.length}`);
  commands.forEach((command, index) => failures.push(...invocationFailures(command, index, context)));
  const restored = sourceSnapshot(context.root, context.spec.sourcePath);
  if (restored.hash !== context.snapshot.hash || !restored.bytes.equals(context.snapshot.bytes) || restored.mode !== context.snapshot.mode) failures.push("production source bytes/hash/mode were not restored exactly");
  return failures;
}

export function runMutation(binary, root, spec) {
  enableMutation(root);
  const snapshot = sourceSnapshot(root, spec.sourcePath);
  const testSnapshot = sourceSnapshot(root, spec.testPath);
  const harness = installHarness(root, spec, snapshot, testSnapshot);
  const result = runProcess({ binary, args: ["mutate", "--scoped", spec.scope, "--max-mutants", "1", "--timeout", "10", "--format", "json"], cwd: root, env: harness.env, timeout: 30_000 });
  const commands = readCommands(harness.log);
  const processError = mutationProcessFailure(result);
  const earlyFailure = earlyMutationFailure(processError, result, commands);
  if (earlyFailure) return earlyFailure;
  const parsed = parseMutationReport(result, commands);
  if (parsed.failure) return parsed.failure;
  if (processError) return failureResult("mutation-report-failed", `mutation process/report status mismatch: ${processError[1]}`, result, { commands });
  const summaryFailure = mutationSummaryFailure(parsed.report, result, commands);
  if (summaryFailure) return summaryFailure;
  const failures = mutationEvidenceFailures(parsed.report, commands, { harness, snapshot, testSnapshot, spec, root });
  if (failures.length) return failureResult("mutation-evidence-mismatch", failures.join("; "), result, { commands });
  return { status: "pass", reasonCode: "ok", diagnostics: "", exitCode: result.status, signal: null, timedOut: false, report: parsed.report, commands };
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
    const previous = outcome.diagnostics || outcome.mutation?.diagnostics || outcome.check?.diagnostics || "";
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
    const mutation = testCase.mutation ? runMutation(binary, root, testCase.mutation) : null;
    const status = [check, mutation].some((item) => item?.status === "fail") ? "fail" : "pass";
    outcome = { id: testCase.id, fixture: testCase.fixture, status, requirement: testCase.mutation?.requirement ?? testCase.check?.requirement ?? null, check, mutation, diagnostics: null };
  } catch (error) {
    outcome = { id: testCase.id, fixture: testCase.fixture, status: "fail", requirement: testCase.mutation?.requirement ?? testCase.check?.requirement ?? null, check: null, mutation: null, diagnostics: bounded(error.message) };
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
    const detail = result.diagnostics || result.mutation?.diagnostics || result.check?.diagnostics || "";
    console.log(`${result.status.toUpperCase().padEnd(7)} ${caseLabel(result)}${detail ? ` — ${bounded(detail)}` : ""}`);
  }
  console.log(`consumer matrix: ${report.summary.pass} pass, ${report.summary.pending} pending, ${report.summary.fail} fail`);
}
