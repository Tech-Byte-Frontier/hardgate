// Real runtime acceptance for any installed Hardgate entry point. The fixture
// uses the selected Rust toolchain, no dependencies, and no evidence exemptions.
"use strict";

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { runReleaseProcess } from "./release-process.mjs";

const SOURCE = "pub fn answer() -> u32 {\n    42\n}\n";
const TEST = "#[test]\nfn answer_matches() {\n    assert_eq!(installed_check_fixture::answer(), 42);\n}\n";
const TIMEOUT_MS = 120_000;

function toolDirectory(name) {
  for (const directory of (process.env.PATH ?? "").split(path.delimiter)) {
    if (!directory) continue;
    try {
      fs.accessSync(path.join(directory, name), fs.constants.X_OK);
      return directory;
    } catch { /* try the next entry */ }
  }
  throw new Error(`installed check requires ${name} on the validation PATH`);
}

function inputHashes(root) {
  return Object.fromEntries(["Cargo.toml", "Cargo.lock", "hardgate.toml", "src/lib.rs", "tests/answer_tests.rs"].map((name) =>
    [name, crypto.createHash("sha256").update(fs.readFileSync(path.join(root, name))).digest("hex")]));
}

async function invoke(binary, args, root, env) {
  try {
    return { status: 0, stdout: await runReleaseProcess(binary, args, { cwd: root, env, timeoutMs: TIMEOUT_MS }) };
  } catch (error) {
    if (!Number.isInteger(error.status)) throw error;
    return { status: error.status, stdout: error.stdout, stderr: error.stderr };
  }
}

async function check(binary, root, env, expectedStatus) {
  const before = inputHashes(root);
  const result = await invoke(binary, ["check", "--json"], root, env);
  assert.deepEqual(inputHashes(root), before, "installed check modified fixture inputs");
  assert.equal(result.status, expectedStatus, `installed check exit: ${result.stderr ?? ""}\n${result.stdout}`);
  const report = JSON.parse(result.stdout);
  assert.equal(report.command, "check");
  assert.equal(report.exit_code, expectedStatus);
  assert.equal(report.partial, false, "installed check must run the complete configured acceptance");
  assert.equal(report.accepted, expectedStatus === 0);
  return report;
}

export async function verifyInstalledCheck(binary, { parent = os.tmpdir(), env = process.env } = {}) {
  const temporary = fs.mkdtempSync(path.join(parent, "hardgate-installed-check-"));
  const root = path.join(temporary, "project");
  fs.mkdirSync(root);
  const runtime = { ...env, PATH: `${toolDirectory("cargo")}:${env.PATH ?? "/usr/bin:/bin"}`,
    RUSTUP_HOME: process.env.RUSTUP_HOME ?? path.join(os.homedir(), ".rustup"),
    CARGO_HOME: path.join(temporary, "cargo-home"), CARGO_NET_OFFLINE: "true" };
  delete runtime.HARDGATE_BINARY;
  delete runtime.HARDGATE_BINARY_PATH;
  try {
    fs.mkdirSync(path.join(root, "src"));
    fs.mkdirSync(path.join(root, "tests"));
    fs.writeFileSync(path.join(root, "Cargo.toml"), '[package]\nname = "installed-check-fixture"\nversion = "0.1.0"\nedition = "2024"\n');
    fs.writeFileSync(path.join(root, "src/lib.rs"), SOURCE);
    fs.writeFileSync(path.join(root, "tests/answer_tests.rs"), TEST);
    const lock = await invoke(path.join(toolDirectory("cargo"), "cargo"), ["generate-lockfile", "--offline"], root, runtime);
    assert.equal(lock.status, 0, `fixture lockfile failed: ${lock.stderr ?? ""}`);
    const init = await invoke(binary, ["init", "--preset", "balanced"], root, runtime);
    assert.equal(init.status, 0, `installed init failed: ${init.stderr ?? ""}`);
    const passed = await check(binary, root, runtime, 0);
    for (const name of ["format_check", "lint", "tests"]) {
      assert.ok(passed.execution.engines.some((engine) => engine.id === name && engine.state === "completed"), `installed check did not complete ${name}`);
    }
    fs.writeFileSync(path.join(root, "tests/answer_tests.rs"), TEST.replace(", 42)", ", 41)"));
    const failed = await check(binary, root, runtime, 1);
    assert.ok(failed.orchestration_violations.some((finding) => finding.step === "test" && finding.exit_code !== null), "installed check did not propagate the real test failure");
    return { passed: true, checks: ["policy", "format", "lint", "tests"], testFailurePropagated: true, inputsPreserved: true };
  } finally {
    fs.rmSync(temporary, { recursive: true, force: true });
  }
}

if (process.argv[1]?.endsWith("/installed-check.mjs")) {
  const binary = process.argv[2];
  if (!binary || process.argv.length !== 3) throw new Error("usage: node scripts/installed-check.mjs /absolute/path/to/installed/hardgate");
  console.log(JSON.stringify(await verifyInstalledCheck(path.resolve(binary))));
}
