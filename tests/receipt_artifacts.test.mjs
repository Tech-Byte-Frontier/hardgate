"use strict";

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { CHANNELS } from "../scripts/release-receipt-validation.mjs";
import { selectReceiptArtifacts } from "../scripts/select-receipt-artifacts.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const cli = path.join(root, "scripts", "select-receipt-artifacts.mjs");
const packages = [...CHANNELS.npmPlatforms];
const runId = "33926961536";
const phase = "exact";
const options = { phase, attempt: "2", runId };

function artifact(packageName, attempt, id, overrides = {}) {
  return {
    id,
    name: `release-receipt-${phase}-${packageName}-attempt-${attempt}`,
    expired: false,
    workflow_run: { id: runId },
    ...overrides,
  };
}

function allCurrent(start = 100) {
  return packages.map((packageName, index) => artifact(packageName, 2, String(start + index)));
}

function rejected(artifacts, expected = /receipt/) {
  assert.throws(() => selectReceiptArtifacts(artifacts, options), expected);
}

const oldAndCurrent = [
  artifact(packages[0], 1, "9000"),
  ...packages.slice(1).map((packageName, index) => artifact(packageName, 2, String(200 + index))),
];
assert.deepEqual(selectReceiptArtifacts(oldAndCurrent, options), ["9000", "200", "201", "202", "203", "204"]);
assert.deepEqual(selectReceiptArtifacts([...allCurrent(400), { ...artifact(packages[0], 2, "999"), name: "release-receipt-default-hardgate-linux-x64-attempt-2" }], options), ["400", "401", "402", "403", "404", "405"]);
assert.deepEqual(selectReceiptArtifacts(packages.map((packageName, index) => ({ ...artifact(packageName, 2, String(450 + index)), name: `release-receipt-default-${packageName}-attempt-2` })), { phase: "default", attempt: 2, runId }), ["450", "451", "452", "453", "454", "455"]);
rejected([...allCurrent(500).slice(0, 5)], /missing selected receipt artifact/);
rejected([...allCurrent(600), artifact(packages[0], 2, "700")], /duplicate package attempt/);
rejected([...allCurrent(800), artifact(packages[0], 1, "800")], /duplicate ID/);
rejected([...allCurrent(900), artifact(packages[0], 3, "999")], /future attempt/);
rejected([...allCurrent(1000).filter((_, index) => index !== 1), artifact(packages[1], 1, "9999"), artifact(packages[1], 2, "10002", { expired: true })], /expired/);
rejected(allCurrent(1100).map((item, index) => index === 0 ? { ...item, workflow_run: { id: "7" } } : item), /another run/);
rejected(allCurrent(1200).map((item, index) => index === 0 ? { ...item, payload: "x".repeat(70 * 1024) } : item), /too large/);
rejected(allCurrent(1300).map((item, index) => index === 0 ? { ...item, id: Number.MAX_SAFE_INTEGER + 1 } : item), /positive/);

const directory = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-receipt-artifacts-"));
try {
  const bin = path.join(directory, "bin");
  const fixture = path.join(directory, "pages.json");
  const log = path.join(directory, "gh-log.json");
  const nodeShebang = `#!${process.execPath}\n`;
  fs.mkdirSync(bin);
  const pages = [
    { total_count: 6, artifacts: [artifact(packages[0], 1, "5001"), { id: "5002", name: "ordinary-build", expired: false }] },
    { total_count: 6, artifacts: packages.slice(1).map((packageName, index) => artifact(packageName, 2, String(5100 + index))) },
  ];
  fs.writeFileSync(fixture, JSON.stringify(pages));
  fs.writeFileSync(path.join(bin, "gh"), `${nodeShebang}
const fs = require("node:fs");
const fixture = ${JSON.stringify(fixture)};
const log = ${JSON.stringify(log)};
fs.writeFileSync(log, JSON.stringify({ args: process.argv.slice(2), env: process.env }));
process.stdout.write(fs.readFileSync(fixture, "utf8"));
`);
  fs.chmodSync(path.join(bin, "gh"), 0o755);
  const environment = {
    ...process.env,
    PATH: `${bin}${path.delimiter}${process.env.PATH ?? ""}`,
    GH_TOKEN: "fixture-token",
    GITHUB_TOKEN: "must-not-leak",
    NPM_TOKEN: "must-not-leak",
    ACTIONS_ID_TOKEN_REQUEST_TOKEN: "must-not-leak",
  };
  const result = spawnSync(process.execPath, [cli, "--phase", phase, "--attempt", "2", "--run-id", runId, "--repo", "OWNER/REPO"], {
    cwd: root,
    encoding: "utf8",
    env: environment,
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout, "5001,5100,5101,5102,5103,5104\n");
  assert.equal(result.stderr, "");
  const invocation = JSON.parse(fs.readFileSync(log, "utf8"));
  assert.deepEqual(invocation.args, ["api", "--paginate", "--slurp", "-X", "GET", `repos/OWNER/REPO/actions/runs/${runId}/artifacts?per_page=100`]);
  assert.equal(invocation.env.GH_TOKEN, "fixture-token");
  assert.equal(invocation.env.GH_HOST, "github.com");
  for (const key of ["GITHUB_TOKEN", "NPM_TOKEN", "ACTIONS_ID_TOKEN_REQUEST_TOKEN"]) assert.equal(invocation.env[key], undefined);

  fs.writeFileSync(path.join(bin, "gh"), `${nodeShebang}
process.stderr.write("Authorization: child-secret-value\\n");
process.stdout.write("child-secret-value\\n");
process.exit(1);
`);
  fs.chmodSync(path.join(bin, "gh"), 0o755);
  const failed = spawnSync(process.execPath, [cli, "--phase", phase, "--attempt", "2", "--run-id", runId, "--repo", "OWNER/REPO"], {
    cwd: root,
    encoding: "utf8",
    env: environment,
  });
  assert.equal(failed.status, 1);
  assert.equal(failed.stdout, "");
  assert.equal(failed.stderr, "release receipt artifact selection failed\n");
  assert.doesNotMatch(`${failed.stdout}${failed.stderr}`, /child-secret-value/);

  const unauthenticated = { ...environment };
  delete unauthenticated.GH_TOKEN;
  const noAuth = spawnSync(process.execPath, [cli, "--phase", phase, "--attempt", "2", "--run-id", runId, "--repo", "OWNER/REPO"], {
    cwd: root,
    encoding: "utf8",
    env: unauthenticated,
  });
  assert.equal(noAuth.status, 1);
  assert.equal(noAuth.stdout, "");
  assert.equal(noAuth.stderr, "release receipt artifact selection failed\n");
  assert.doesNotMatch(`${noAuth.stdout}${noAuth.stderr}`, /must-not-leak/);
} finally {
  fs.rmSync(directory, { recursive: true, force: true });
}

console.log("receipt_artifacts.test: OK (attempt selection, strict metadata, bounded CLI, credential-free failures)");
