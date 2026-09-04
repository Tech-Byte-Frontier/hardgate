"use strict";

import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { CONSUMER_CASES, caseLabel } from "./consumer-fixtures.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const FIXTURE_ROOT = path.join(ROOT, "tests", "fixtures", "consumers");
const MANAGERS = ["npm", "pnpm", "yarn", "bun", "npx"];

export function resolveBinary(options) {
  const candidates = [
    options.binary,
    process.env.HARDGATE_BINARY,
    path.join(ROOT, "target", "debug", "hardgate"),
    path.join(ROOT, "target", "release", "hardgate"),
  ].filter(Boolean);
  const binary = candidates.find((candidate) => fs.existsSync(candidate));
  if (!binary) {
    throw new Error(
      "No local Hardgate binary found; run `cargo build --locked --bin hardgate` or set HARDGATE_BINARY.",
    );
  }
  return path.resolve(binary);
}

function copyFixture(testCase) {
  const source = path.join(FIXTURE_ROOT, testCase.fixture);
  if (!fs.existsSync(source)) throw new Error(`fixture is missing: ${testCase.fixture}`);
  const target = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-consumer-"));
  fs.cpSync(source, target, { recursive: true });
  return target;
}

function runProcess({ binary, args, cwd, env = {}, timeout = 30_000 }) {
  const child = spawnSync(binary, args, {
    cwd,
    env: { ...process.env, ...env },
    encoding: "utf8",
    timeout,
  });
  return {
    status: child.status,
    signal: child.signal,
    stdout: child.stdout ?? "",
    stderr: child.stderr ?? "",
    error: child.error?.message ?? null,
  };
}

function parseReport(stdout) {
  const first = stdout.indexOf("{");
  const last = stdout.lastIndexOf("}");
  if (first < 0 || last <= first) throw new Error("Hardgate did not emit a JSON report");
  try {
    return JSON.parse(stdout.slice(first, last + 1));
  } catch (error) {
    throw new Error(`invalid Hardgate JSON report: ${error.message}`);
  }
}

function runCheck(binary, root, expectation, diff = false) {
  const args = ["check", "--format", "json"];
  if (diff) args.push("--diff");
  const result = runProcess({ binary, args, cwd: root });
  let report;
  try {
    report = parseReport(result.stdout);
  } catch (error) {
    return {
      status: "fail",
      exitCode: result.status,
      diagnostics: `${error.message}; stdout=${result.stdout}; stderr=${result.stderr}`,
    };
  }
  const failures = checkFailures(report, result, expectation);
  return {
    status: failures.length === 0 ? "pass" : "fail",
    exitCode: result.status,
    report,
    diagnostics: failures.join("; "),
    stdout: result.stdout,
    stderr: result.stderr,
  };
}

function checkFailures(report, result, expectation) {
  const passed = result.status === 0 && report.passed === true;
  const alternate = expectation.allowPassWithNoUnsupported && passed;
  const failures = outcomeFailures(expectation, passed, result, alternate);
  failures.push(...advisoryFailures(report, expectation));
  if (!alternate) failures.push(...evidenceFailures(report, expectation));
  failures.push(...metricFailures(report, expectation));
  return failures;
}

function outcomeFailures(expectation, passed, result, alternate) {
  if (expectation.expectPass === passed || alternate) return [];
  return [`expected ${expectation.expectPass ? "pass" : "failure"}, observed exit=${result.status} passed=${passed}`];
}

function advisoryFailures(report, expectation) {
  return (expectation.advisoryIncludes ?? []).filter((text) => !report.advisories.some((advisory) => advisory.includes(text))).map((text) => `missing advisory containing ${JSON.stringify(text)}`);
}

function evidenceFailures(report, expectation) {
  const failures = (expectation.orchestrationSteps ?? []).filter((step) => !report.orchestration_violations.some((violation) => violation.step === step)).map((step) => `missing orchestration evidence step ${JSON.stringify(step)}`);
  for (const target of expectation.paths ?? []) {
    const found = report.orchestration_violations.some((violation) => violation.command.includes(target) || violation.output.includes(target));
    if (!found) failures.push(`missing evidence target ${JSON.stringify(target)}`);
  }
  return failures;
}

function metricFailures(report, expectation) {
  const failures = [];
  if (expectation.minFiles && report.files_scanned < expectation.minFiles) failures.push(`expected at least ${expectation.minFiles} inventoried files, got ${report.files_scanned}`);
  if (expectation.minFunctions && report.functions_analyzed < expectation.minFunctions) failures.push(`expected at least ${expectation.minFunctions} parsed functions, got ${report.functions_analyzed}`);
  return failures;
}

function initializeFixture(binary, root, preset) {
  const result = runProcess({ binary, args: ["init", "--preset", preset], cwd: root });
  if (result.status !== 0 || !fs.existsSync(path.join(root, "hardgate.toml"))) throw new Error(`hardgate init failed: ${result.stdout}${result.stderr}`);
  const config = fs.readFileSync(path.join(root, "hardgate.toml"), "utf8");
  if (!config.includes('preset = "strict-agent"')) throw new Error("strict init did not write preset = \"strict-agent\"");
  if (!config.includes("strict = true") || config.includes('"tests/**"')) throw new Error("strict init must keep strict policy and avoid a tests/** exclusion");
}

function prepareLegacyReference(root) {
  for (const args of [["init", "-q"], ["config", "user.email", "hardgate@example.invalid"], ["config", "user.name", "Hardgate Consumer Fixture"], ["config", "commit.gpgsign", "false"], ["add", "-A"], ["commit", "-qm", "legacy baseline"], ["branch", "-M", "main"], ["switch", "-q", "-c", "consumer-change"]]) git(root, args);
  const source = path.join(root, "src", "legacy.ts");
  fs.writeFileSync(source, fs.readFileSync(source, "utf8").replace("legacy(first: string, second: string)", "legacy(first: string, second: string, third: string)"));
}

function git(cwd, args) {
  execFileSync("git", args, { cwd, stdio: "ignore" });
}

function enableMutation(root) {
  const configPath = path.join(root, "hardgate.toml");
  const config = fs.readFileSync(configPath, "utf8");
  const enabled = config.replace(/(\[mutation\]\s*\n\s*enabled\s*=\s*)false/, "$1true");
  if (enabled === config) throw new Error("fixture config has no disabled mutation section");
  fs.writeFileSync(configPath, enabled);
}

function createCommandShims(root) {
  const bin = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-consumer-bin-"));
  const log = path.join(root, ".consumer-command-log");
  for (const manager of MANAGERS) {
    const script = [
      "#!/bin/sh",
      'if [ -n "$HARDGATE_CONSUMER_COMMAND_LOG" ]; then',
      '  printf "%s|%s|%s\\n" "$PWD" "$(basename "$0")" "$*" >> "$HARDGATE_CONSUMER_COMMAND_LOG"',
      "fi",
      "exit 0",
      "",
    ].join("\n");
    fs.writeFileSync(path.join(bin, manager), script, { mode: 0o755 });
  }
  return { log, env: { PATH: `${bin}${path.delimiter}${process.env.PATH ?? ""}`, HARDGATE_CONSUMER_COMMAND_LOG: log } };
}

function readCommands(log) {
  if (!fs.existsSync(log)) return [];
  return fs.readFileSync(log, "utf8").trim().split("\n").filter(Boolean).map((line) => {
    const [cwd, manager, ...rest] = line.split("|");
    return { cwd, manager, args: rest.join("|") };
  });
}

function runMutation(binary, root, expectation) {
  enableMutation(root);
  const shims = createCommandShims(root);
  const args = [
    "mutate",
    "--scoped",
    expectation.scope,
    "--max-mutants",
    "1",
    "--timeout",
    "1",
    "--format",
    "json",
  ];
  const result = runProcess({ binary, args, cwd: root, env: shims.env, timeout: 20_000 });
  const commands = readCommands(shims.log);
  const matching = commands.find((command) => {
    if (command.manager !== expectation.manager) return false;
    const rendered = `${command.manager} ${command.args}`;
    return (expectation.includes ?? []).every((term) => rendered.includes(term)) && (!expectation.cwdSuffix || command.cwd.endsWith(expectation.cwdSuffix));
  });
  if (matching) {
    return {
      status: "pass",
      exitCode: result.status,
      command: matching,
      commands,
      stdout: result.stdout,
      stderr: result.stderr,
    };
  }
  return {
    status: "pending",
    exitCode: result.status,
    commands,
    diagnostics: `resolved command did not match manager=${expectation.manager} terms=${JSON.stringify(expectation.includes ?? [])}`,
    stdout: result.stdout,
    stderr: result.stderr,
    requirement: expectation.requirement,
  };
}

function checkLegacyText(result, expectation) {
  if (result.status !== "pass") return result;
  const report = result.report ?? {};
  const evidence = [...(report.advisories ?? []), ...(report.orchestration_violations ?? []).flatMap((item) => [item.step, item.output, item.recommendation]), ...(report.complexity_violations ?? []).flatMap((item) => [item.message, item.recommendation]), ...(report.budget_violations ?? []).flatMap((item) => [item.metric, item.message])];
  if (!expectation.requireText.test(`${evidence.join("\n")}\n${result.stderr}`)) return { status: "pending", diagnostics: "legacy run has no baseline/reference/ratchet evidence", requirement: expectation.requirement };
  return result;
}

function prepareCase(binary, testCase) {
  const root = copyFixture(testCase);
  if (testCase.initialize) initializeFixture(binary, root, testCase.initialize);
  if (testCase.legacy) prepareLegacyReference(root);
  return root;
}

function caseStatus(check, mutation) {
  const results = [check, mutation].filter(Boolean);
  if (results.some((result) => result.status === "fail")) return "fail";
  if (results.some((result) => result.status === "pending")) return "pending";
  return "pass";
}

function runCase(binary, testCase, keepTemp) {
  let root;
  try {
    root = prepareCase(binary, testCase);
    const check = runCheck(binary, root, testCase.check, Boolean(testCase.legacy));
    const checked = testCase.legacy ? checkLegacyText(check, testCase.check) : check;
    const mutation = testCase.mutation ? runMutation(binary, root, testCase.mutation) : null;
    return {
      id: testCase.id,
      fixture: testCase.fixture,
      status: caseStatus(checked, mutation),
      requirement: testCase.mutation?.requirement ?? testCase.check?.requirement,
      check: checked,
      mutation,
      tempRoot: keepTemp ? root : undefined,
    };
  } catch (error) {
    return { id: testCase.id, fixture: testCase.fixture, status: "fail", diagnostics: error.message };
  } finally {
    if (root && !keepTemp) fs.rmSync(root, { recursive: true, force: true });
  }
}

export function runConsumerMatrix(options) {
  const binary = resolveBinary(options);
  const selected = options.caseIds.length === 0
    ? CONSUMER_CASES
    : CONSUMER_CASES.filter((testCase) => options.caseIds.includes(testCase.id));
  if (selected.length === 0) throw new Error("no consumer fixtures matched --case");
  const cases = selected.map((testCase) => runCase(binary, testCase, options.keepTemp));
  const summary = cases.reduce(
    (counts, result) => ({ ...counts, [result.status]: counts[result.status] + 1 }),
    { pass: 0, pending: 0, fail: 0 },
  );
  return { binary, allowPending: options.allowPending, cases, summary };
}

export function renderHuman(report) {
  for (const result of report.cases) {
    const detail = result.diagnostics || result.mutation?.diagnostics || result.check?.diagnostics || "";
    console.log(`${result.status.toUpperCase().padEnd(7)} ${caseLabel(result)}${detail ? ` — ${detail}` : ""}`);
    if (result.status === "pending" && result.requirement) console.log(`         requires: ${result.requirement}`);
  }
  console.log(`consumer matrix: ${report.summary.pass} pass, ${report.summary.pending} pending, ${report.summary.fail} fail`);
}
