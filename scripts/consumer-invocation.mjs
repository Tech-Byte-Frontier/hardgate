"use strict";

import fs from "node:fs";

function expectedPathBins(harness) {
  const bins = [fs.realpathSync(harness.packageBin)];
  if (harness.workspaceBin !== harness.packageBin) bins.push(fs.realpathSync(harness.workspaceBin));
  return bins;
}

function pathMatches(command, expectedBins) {
  const prefix = Array.isArray(command.pathEntries) ? command.pathEntries.slice(0, expectedBins.length) : [];
  return command.pathBinsExpected
    && JSON.stringify(command.pathBins) === JSON.stringify(expectedBins)
    && JSON.stringify(prefix) === JSON.stringify(expectedBins);
}

export function invocationIdentityFailures(command, position, harness, spec) {
  const failures = [];
  const expectedBins = expectedPathBins(harness);
  if (command.manager !== spec.manager || JSON.stringify(command.argv) !== JSON.stringify(spec.argv)) failures.push(`invocation ${position} manager/argv mismatch`);
  if (command.cwd !== harness.packageRoot) failures.push(`invocation ${position} CWD mismatch`);
  if (command.executable !== fs.realpathSync(harness.managerPath)) failures.push(`invocation ${position} did not use package-local .bin`);
  if (command.packageRoot !== harness.packageRoot || command.workspaceRoot !== harness.workspaceRoot) failures.push(`invocation ${position} workspace provenance mismatch`);
  if (!pathMatches(command, expectedBins)) failures.push(`invocation ${position} PATH must start with package-local then workspace-local .bin only`);
  return failures;
}
