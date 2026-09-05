#!/usr/bin/env node
// Verify exact npm pack archives through real npm and pnpm consumers.
"use strict";

import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { inspectPackedArtifacts, snapshotArchiveFiles, verifyArchiveSnapshot } from "./packed-consumer-artifacts.mjs";
import { startLocalRegistry } from "./packed-consumer-registry.mjs";
import { createConsumerRoot, expectedVersion, installAndVerify } from "./packed-consumer-runtime.mjs";

export function aggregateCleanupErrors(primaryError, cleanupErrors) {
  if (!primaryError && cleanupErrors.length === 0) return null;
  if (!primaryError && cleanupErrors.length === 1) return cleanupErrors[0];
  if (!primaryError) return new AggregateError(cleanupErrors, "packed consumer cleanup failed");
  if (cleanupErrors.length === 0) return primaryError;
  return new AggregateError([primaryError, ...cleanupErrors], "packed consumer check and cleanup failed", { cause: primaryError });
}

async function finalizePackedCheck({ snapshot, registry, tempRoot, result, primaryError }) {
  const cleanupErrors = [];
  try {
    verifyArchiveSnapshot(snapshot);
  } catch (error) {
    cleanupErrors.push(error);
  }
  try {
    if (registry) await registry.close();
  } catch (error) {
    cleanupErrors.push(error);
  }
  try {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  } catch (error) {
    cleanupErrors.push(error);
  }
  const failure = aggregateCleanupErrors(primaryError, cleanupErrors);
  if (failure) throw failure;
  return result;
}

export async function checkPackedConsumers({ packagesDir, binary, version }) {
  const inspected = inspectPackedArtifacts(packagesDir, version, binary);
  const snapshot = snapshotArchiveFiles(inspected.artifacts);
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-packed-consumers-"));
  let registry;
  let result;
  let primaryError;
  try {
    const expectedOutput = await expectedVersion(inspected.expectedBinary, inspected.expectedVersion, tempRoot);
    registry = await startLocalRegistry(inspected.artifacts);
    const consumers = [];
    for (const manager of ["npm", "pnpm"]) {
      const root = createConsumerRoot(tempRoot, manager);
      consumers.push(await installAndVerify({
        manager,
        root,
        registry,
        version: inspected.expectedVersion,
        host: inspected.host,
        wrapperLauncherBytes: inspected.wrapperLauncherBytes,
        expectedOutput,
        expectedHash: inspected.expectedHash,
        tempRoot,
      }));
    }
    verifyArchiveSnapshot(snapshot);
    result = {
      version: inspected.expectedVersion,
      hostPackage: inspected.host,
      binarySha256: inspected.expectedHash,
      expectedVersionOutput: expectedOutput,
      archives: [...inspected.artifacts.values()].map((artifact) => ({
        name: artifact.name,
        version: artifact.version,
        sha256: artifact.archiveSha256,
      })),
      consumers,
      registryRequests: registry.requests,
    };
  } catch (error) {
    primaryError = error;
  }
  return finalizePackedCheck({ snapshot, registry, tempRoot, result, primaryError });
}

function parseArgs(argv) {
  const options = {};
  const keys = { "--packages-dir": "packagesDir", "--binary": "binary", "--version": "version" };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--help" || argument === "-h") {
      options.help = true;
      continue;
    }
    if (argument === "--json") {
      options.json = true;
      continue;
    }
    if (Object.hasOwn(keys, argument)) {
      const value = argv[++index];
      if (!value || value.startsWith("--")) throw new Error(`${argument} requires a value`);
      options[keys[argument]] = value;
      continue;
    }
    const match = argument.match(/^(--packages-dir|--binary|--version)=(.*)$/);
    if (match) {
      if (!match[2]) throw new Error(`${match[1]} requires a value`);
      options[keys[match[1]]] = match[2];
      continue;
    }
    throw new Error(`unknown argument: ${argument}`);
  }
  return options;
}

function printHelp() {
  console.log("Usage: node scripts/check-packed-consumers.mjs [--json] --packages-dir DIR --binary PATH --version VERSION");
  console.log("Serve only the supplied .tgz archives from localhost, then verify npm and pnpm installs.");
}

function renderReport(report) {
  console.log(`packed consumers: OK (${report.version})`);
  console.log(`host optional dependency: ${report.hostPackage}`);
  console.log(`native binary sha256: ${report.binarySha256}`);
  for (const consumer of report.consumers) console.log(`${consumer.manager}: ${consumer.versionOutput} (${consumer.nativeSha256})`);
}

async function main(argv = process.argv.slice(2)) {
  const options = parseArgs(argv);
  if (options.help) {
    printHelp();
    return 0;
  }
  const report = await checkPackedConsumers(options);
  if (options.json) console.log(JSON.stringify(report, null, 2));
  else renderReport(report);
  return 0;
}

if (process.argv[1]?.endsWith("/check-packed-consumers.mjs")) {
  main().catch((error) => {
    console.error(`packed consumer check error: ${error.message}`);
    process.exitCode = 2;
  });
}
