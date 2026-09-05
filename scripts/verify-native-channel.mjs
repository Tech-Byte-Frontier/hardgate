#!/usr/bin/env node
// Verify one published native npm package and, on Linux x64 GNU, its wrapper.
// Usage: node scripts/verify-native-channel.mjs --package NAME --version V
//   --source-sha SHA --archive FILE --mode exact|default --output PROOF.json
//   [--wrapper-source SIGNED_WRAPPER.js]
"use strict";

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { writeProofAtomic } from "./native-channel-proof.mjs";
import { regularFile, verifyNativeArchive } from "./native-channel-archive.mjs";
import {
  channelProof,
  installNpmPackage,
  verifyDirectConsumer,
  verifyWrapperConsumer,
} from "./native-channel-install.mjs";
import { verificationPolicy } from "./npm-verification-policy.mjs";
import { runReleaseProcess } from "./release-process.mjs";
import {
  NATIVE_PACKAGES,
  PROOF_VERSION,
  PUBLIC_NPM_REGISTRY,
  WRAPPER_PACKAGE,
  assertHostSupports,
  assertSourceSha,
  assertVersion,
  detectHost,
  digestFile,
  fail,
  nodeNpmPath,
  packageDescriptor,
  parseArgs,
  wrapperHost,
} from "./native-channel-support.mjs";

function requestMode(request) {
  const mode = request?.mode;
  if (mode !== "exact" && mode !== "default") fail(`mode must be exact or default, got ${mode || "<missing>"}`);
  return mode;
}

function requestArchive(request, packageName) {
  if (typeof request?.archive !== "string" || request.archive.length === 0) fail("archive is required");
  const archive = path.resolve(request.archive);
  if (path.basename(archive) !== `${packageName}.tar.gz`) fail(`archive must be named ${packageName}.tar.gz`);
  return archive;
}

function requestOutput(request, archive) {
  if (request.output !== undefined && (typeof request.output !== "string" || request.output.length === 0)) {
    fail("output must be a non-empty path");
  }
  const output = request.output === undefined ? undefined : path.resolve(request.output);
  if (output !== undefined && archive === output) fail("archive and output must identify different files");
  return output;
}

function requestWrapperSource(request) {
  const wrapperSource = request.wrapperSource ?? request.wrapper_source;
  if (wrapperSource !== undefined && (typeof wrapperSource !== "string" || wrapperSource.length === 0)) {
    fail("wrapper-source must be a non-empty path");
  }
  if (typeof wrapperSource === "string" && wrapperSource.includes("\0")) fail("wrapper-source cannot contain NUL bytes");
  return wrapperSource === undefined ? undefined : path.resolve(wrapperSource);
}

function requestValues(request) {
  const packageName = request?.packageName ?? request?.package;
  const version = assertVersion(request?.version);
  const sourceSha = assertSourceSha(request?.sourceSha ?? request?.source_sha);
  const descriptor = packageDescriptor(packageName);
  const mode = requestMode(request);
  const archive = requestArchive(request, descriptor.name);
  const output = requestOutput(request, archive);
  return {
    packageName: descriptor.name,
    descriptor,
    version,
    sourceSha,
    mode,
    archive,
    output,
    wrapperSource: requestWrapperSource(request),
  };
}

function assertArchiveEvidence(archiveEvidence) {
  if (!archiveEvidence || typeof archiveEvidence.sha256 !== "string" || !/^[0-9a-f]{64}$/.test(archiveEvidence.sha256)) {
    fail("archive verifier did not return a lowercase SHA256 digest");
  }
  return archiveEvidence;
}

async function verifyChannelWork({ values, host, policy, runProcess, verifyArchive, install, workRoot, canonicalWrapper }) {
  const archiveEvidence = assertArchiveEvidence(await verifyArchive({
    archive: values.archive,
    packageName: values.packageName,
    version: values.version,
    sourceSha: values.sourceSha,
    descriptor: values.descriptor,
    directory: workRoot,
    runProcess,
    policy,
  }));
  const archiveSha256 = digestFile(values.archive);
  const { direct, expectedOutput } = await verifyDirectConsumer({
    values,
    host,
    policy,
    workRoot,
    install,
    runProcess,
    archiveEvidence,
  });
  const wrapper = canonicalWrapper ? await verifyWrapperConsumer({
    values,
    host,
    policy,
    workRoot,
    install,
    runProcess,
    archiveEvidence,
    expectedOutput,
  }) : undefined;
  return channelProof({ values, direct, archiveSha256, wrapper });
}

function wrapperConfiguration(values, host) {
  const canonicalWrapper = wrapperHost(host) && values.packageName === "hardgate-linux-x64";
  if (canonicalWrapper && values.wrapperSource === undefined) fail("--wrapper-source is required for the canonical Linux x64 GNU wrapper verification");
  if (!canonicalWrapper && values.wrapperSource !== undefined) fail("--wrapper-source is only valid for the canonical Linux x64 GNU wrapper verification");
  if (canonicalWrapper) regularFile(values.wrapperSource, "--wrapper-source");
  return canonicalWrapper;
}

async function cleanupFailure(cleanup, workRoot) {
  try {
    await cleanup(workRoot);
  } catch (error) {
    return error;
  }
  return undefined;
}

function preserveFailure(primaryFailure, cleanupFailureValue) {
  return primaryFailure ?? cleanupFailureValue;
}

export async function verifyNativeChannel(request, options = {}) {
  const values = requestValues(request);
  const host = options.host ?? detectHost();
  assertHostSupports(values.descriptor, host);
  regularFile(values.archive, "--archive");
  const policy = options.policy ?? verificationPolicy(values.version);
  const runProcess = options.runProcess ?? runReleaseProcess;
  const canonicalWrapper = wrapperConfiguration(values, host);
  const workRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-native-channel-"));
  const verifyArchive = options.verifyArchive ?? verifyNativeArchive;
  const install = options.installPackage ?? ((args) => installNpmPackage({ ...args, runProcess, npmCommand: options.npmCommand ?? nodeNpmPath() }));
  const cleanup = options.cleanup ?? ((directory) => fs.rmSync(directory, { recursive: true, force: true }));
  let proof;
  let failure;
  try {
    proof = await verifyChannelWork({
      values,
      host,
      policy,
      runProcess,
      verifyArchive,
      install,
      workRoot,
      canonicalWrapper,
    });
  } catch (error) {
    failure = error;
  }
  failure = preserveFailure(failure, await cleanupFailure(cleanup, workRoot));
  if (failure) throw failure;
  if (values.output !== undefined) writeProofAtomic(values.output, proof);
  return proof;
}

async function main(argv = process.argv.slice(2)) {
  const request = parseArgs(argv);
  await verifyNativeChannel(request);
  console.log(`verified ${request.packageName} ${request.mode} consumer channel`);
}

const invokedPath = process.argv[1];
if (invokedPath && import.meta.url === pathToFileURL(invokedPath).href) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
}

export { NATIVE_PACKAGES, PROOF_VERSION, PUBLIC_NPM_REGISTRY, WRAPPER_PACKAGE, installNpmPackage, verifyNativeArchive };
