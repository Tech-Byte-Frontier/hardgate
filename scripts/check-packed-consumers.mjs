#!/usr/bin/env node
// Verify exact npm pack archives through real npm and pnpm consumers.
"use strict";

import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { inspectPackedArtifacts, snapshotArchiveFiles, verifyArchiveSnapshot } from "./packed-consumer-artifacts.mjs";
import { startLocalRegistry } from "./packed-consumer-registry.mjs";
import { installAndVerifyGlobal } from "./packed-consumer-global.mjs";
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
    for (const manager of ["npm", "pnpm"]) {
      const root = createConsumerRoot(tempRoot, `${manager}-global`);
      consumers.push(await installAndVerifyGlobal({ manager, root, registry, version: inspected.expectedVersion,
        expectedOutput, expectedHash: inspected.expectedHash, expectedBinary: inspected.expectedBinary,
        wrapperLauncherBytes: inspected.wrapperLauncherBytes, tempRoot }));
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

const VALUE_OPTIONS = { "--packages-dir": "packagesDir", "--binary": "binary", "--version": "version" };

function inlineOption(argument, options) {
  const match = argument.match(/^(--packages-dir|--binary|--version)=(.*)$/);
  if (!match) return false;
  if (!match[2]) throw new Error(`${match[1]} requires a value`);
  options[VALUE_OPTIONS[match[1]]] = match[2];
  return true;
}

function parseArgument(argument, next, options) {
  if (argument === "--help" || argument === "-h") {
    options.help = true;
    return 0;
  }
  if (argument === "--json") {
    options.json = true;
    return 0;
  }
  if (inlineOption(argument, options)) return 0;
  if (!Object.hasOwn(VALUE_OPTIONS, argument)) throw new Error(`unknown argument: ${argument}`);
  if (!next || next.startsWith("--")) throw new Error(`${argument} requires a value`);
  options[VALUE_OPTIONS[argument]] = next;
  return 1;
}

function parseArgs(argv) {
  const options = {};
  for (let index = 0; index < argv.length; index += 1) index += parseArgument(argv[index], argv[index + 1], options);
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
  for (const consumer of report.consumers) console.log(`${consumer.manager} (${consumer.scope}): ${consumer.versionOutput} (${consumer.nativeSha256})`);
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
