"use strict";

import fs from "node:fs";
import path from "node:path";

function expectedPathBins(harness, rebase) {
  const bins = [rebase(harness.packageBin)];
  if (harness.workspaceBin !== harness.packageBin) bins.push(rebase(harness.workspaceBin), harness.workspaceBin);
  return bins;
}

function pathMatches(command, expectedBins) {
  const prefix = Array.isArray(command.pathEntries) ? command.pathEntries.slice(0, expectedBins.length) : [];
  return command.pathBinsExpected
    && JSON.stringify(command.pathBins) === JSON.stringify(expectedBins)
    && JSON.stringify(prefix) === JSON.stringify(expectedBins);
}

function isolatedRoot(root, original) {
  return typeof root === "string" && path.isAbsolute(root)
    && path.relative(original, root).startsWith(`..${path.sep}`);
}

export function invocationIdentityFailures(command, position, harness, spec) {
  const failures = [];
  const isolated = command.isolatedRoot;
  if (!isolatedRoot(isolated, harness.root)) {
    return [`invocation ${position} did not execute outside the original workspace`];
  }
  if (fs.existsSync(isolated)) failures.push(`invocation ${position} left its mutation workspace behind`);
  const rebase = (value) => path.resolve(isolated, path.relative(harness.root, value));
  const expectedBins = expectedPathBins(harness, rebase);
  if (command.manager !== spec.manager || JSON.stringify(command.argv) !== JSON.stringify(spec.argv)) failures.push(`invocation ${position} manager/argv mismatch`);
  if (command.cwd !== rebase(harness.packageRoot)) failures.push(`invocation ${position} CWD mismatch`);
  if (command.executable !== rebase(harness.managerPath)) failures.push(`invocation ${position} did not use isolated package-local .bin`);
  if (command.packageRoot !== rebase(harness.packageRoot) || command.workspaceRoot !== rebase(harness.workspaceRoot)) failures.push(`invocation ${position} workspace provenance mismatch`);
  if (!pathMatches(command, expectedBins)) failures.push(`invocation ${position} PATH must retain isolated package/workspace precedence before the explicit inherited bin`);
  return failures;
}
