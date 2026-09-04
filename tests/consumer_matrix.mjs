// Offline black-box acceptance gate. Run after building Hardgate:
//   cargo build --locked --bin hardgate
//   node tests/consumer_matrix.mjs
"use strict";

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const runner = path.join(root, "scripts", "check-consumer-matrix.mjs");
const result = spawnSync(process.execPath, [runner, "--json"], {
  cwd: root,
  encoding: "utf8",
  env: { ...process.env, HARDGATE_CONSUMER_MATRIX: "ci" },
});

assert.equal(
  result.status,
  0,
  `consumer acceptance matrix is not green (status=${result.status})\n${result.stdout}\n${result.stderr}\n${result.error?.message ?? ""}`,
);

const report = JSON.parse(result.stdout);
assert.ok(report.cases?.length >= 11, "matrix must exercise every stabilization category");
assert.equal(report.summary.pending, 0, "CI mode cannot leave pending consumer capabilities");
assert.equal(report.summary.fail, 0, "CI mode cannot leave failed consumer fixtures");
console.log(`consumer_matrix: ${report.summary.pass} fixtures passed`);
