// Portable native/npm consumer checks, including ordinary tools without Rust.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";

export function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: "utf8", timeout: 120_000, ...options });
  assert.ifError(result.error);
  assert.equal(result.status, 0, `${command}: ${result.stderr}\n${result.stdout}`);
  return result.stdout;
}

export function npm(args, options = {}) {
  const prefix = path.dirname(process.execPath);
  const candidates = [
    path.join(prefix, "node_modules/npm/bin/npm-cli.js"),
    path.join(prefix, "../lib/node_modules/npm/bin/npm-cli.js"),
    path.join(prefix, "../share/nodejs/npm/bin/npm-cli.js"),
  ];
  const cli = candidates.find((candidate) => fs.existsSync(candidate));
  assert.ok(cli, "npm-cli.js must be installed alongside Node");
  return run(process.execPath, [cli, ...args], options);
}

export function verifyLocalAnalysis(command, prefix = [], env = process.env) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-local-consumer-"));
  try {
    fs.mkdirSync(path.join(root, "src"));
    fs.writeFileSync(path.join(root, "src/lib.rs"), "pub fn answer() -> u32 { 42 }\n");
    fs.writeFileSync(path.join(root, "hardgate.toml"), "[gate]\npreset = 'custom'\n[budgets.functions]\nmax_parameters = 1\n");
    const invoke = (args, status = 0) => {
      const staticEnv = { ...env, PATH: "", Path: "" };
      const result = spawnSync(command, [...prefix, ...args], { cwd: root, env: staticEnv, encoding: "utf8", timeout: 30_000 });
      assert.ifError(result.error);
      assert.equal(result.status, status, `${args}: ${result.stderr}\n${result.stdout}`);
      return JSON.parse(result.stdout);
    };
    assert.equal(invoke(["scan", "src/lib.rs", "--json"]).passed, true);
    const report = invoke(["check", "--checks", "policy", "--json", "--report-json", "gate.json"]);
    assert.equal(report.partial, true);
    assert.equal(report.accepted, false);
    invoke(["report", "gate.json", "--json"]);
    fs.writeFileSync(path.join(root, "src/lib.rs"), "pub fn add(a: u32, b: u32) -> u32 { a + b }\n");
    assert.equal(invoke(["scan", "src/lib.rs", "--json"], 1).passed, false);
    fs.writeFileSync(path.join(root, "src/lib.rs"), "pub fn answer() -> u32 { 42 }\n");
    const policy = "[gate]\npreset = 'custom'\n[orchestration]\nformat_check = '/bin/sh check.sh'\nlint = '/bin/sh check.sh'\n";
    fs.writeFileSync(path.join(root, "hardgate.toml"), policy);
    fs.writeFileSync(path.join(root, "check.sh"), "test -f src/lib.rs || exit 7\necho native-tool-ran\n");
    const complete = invoke(["check", "--json"]);
    assert.equal(complete.accepted, true);
    assert.equal(complete.partial, false);
    fs.writeFileSync(path.join(root, "check.sh"), "echo native-tool-failed; exit 7\n");
    assert.equal(invoke(["check", "--json"], 1).accepted, false);
    if (process.platform !== "linux") {
      fs.writeFileSync(path.join(root, "hardgate.toml"), `${policy}require_isolation = true\n`);
      assert.match(invoke(["check", "--json"], 2).message, /requires Linux cgroup v2/);
    }
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
}
