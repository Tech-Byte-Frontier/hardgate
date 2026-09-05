"use strict";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { projectRoot } from "../scripts/release-support.mjs";

const directory = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-auth-preflight-"));
const log = path.join(directory, "operations");
const npm = path.join(directory, "npm");
fs.writeFileSync(npm, `#!${process.execPath}
import fs from 'node:fs';
fs.appendFileSync(process.env.PREFLIGHT_LOG, process.argv[2] + '\\n');
if (process.argv[2] === '--version') {
  if (process.env.NODE_AUTH_TOKEN || process.env.ACTIONS_ID_TOKEN_REQUEST_TOKEN) throw new Error('version credential leak');
  console.log('12.0.2');
} else if (process.argv[2] === 'whoami') {
  if (process.env.NODE_AUTH_TOKEN !== 'fixture-preflight-token') throw new Error('auth unavailable');
  console.log('fixture-publisher');
} else throw new Error('unexpected operation');
`, { mode: 0o755 });

function run(mode, credentials) {
  fs.writeFileSync(log, "");
  const result = spawnSync(process.execPath, [path.join(projectRoot, "scripts/npm-publisher-preflight.mjs"), "--auth-mode", mode], {
    encoding: "utf8", timeout: 10_000,
    env: { PATH: `${directory}:${path.dirname(process.execPath)}:/usr/bin:/bin`, PREFLIGHT_LOG: log, ...credentials },
  });
  assert.doesNotMatch(result.stdout + result.stderr, /fixture-preflight-token|fixture-preflight-oidc|fixture-publisher/);
  return { result, calls: fs.readFileSync(log, "utf8").trim().split("\n").filter(Boolean) };
}

try {
  const token = run("token", { NODE_AUTH_TOKEN: "fixture-preflight-token" });
  assert.equal(token.result.status, 0, token.result.stderr);
  assert.deepEqual(token.calls, ["--version", "whoami"]);
  const trusted = run("trusted", { GITHUB_ACTIONS: "true", NODE_AUTH_TOKEN: "fixture-preflight-token", ACTIONS_ID_TOKEN_REQUEST_TOKEN: "fixture-preflight-oidc", ACTIONS_ID_TOKEN_REQUEST_URL: "https://oidc.example.test/request" });
  assert.equal(trusted.result.status, 0, trusted.result.stderr);
  assert.deepEqual(trusted.calls, ["--version"]);
  assert.match(trusted.result.stdout, /binding is checked by publication/);
  for (const mode of ["token", "trusted", "invalid"]) {
    const rejected = run(mode, {});
    assert.equal(rejected.result.status, 1);
    assert.deepEqual(rejected.calls, []);
  }
} finally {
  fs.rmSync(directory, { recursive: true, force: true });
}
console.log("npm_publisher_preflight.test: OK");
