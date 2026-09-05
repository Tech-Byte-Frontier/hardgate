#!/usr/bin/env node
// Verify that the public crates.io archive is the exact local Cargo archive.
// Usage: node scripts/verify-crate-publication.mjs --expected <file> --version <version> --source-sha <sha> [--require-default]
"use strict";

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { childTimeoutMs, verificationPolicy } from "./npm-verification-policy.mjs";
import {
  CURL_COMMAND,
  MAX_API_OUTPUT_BYTES,
  TAR_COMMAND,
  VerificationError,
  apiRequestArgs,
  assertExpectedPath,
  assertSourceSha,
  assertVersion,
  defaultRunner,
  digest,
  fail,
  parseArguments,
  parseCurlResponse,
  readCargoVcsInfo,
  readStableFile,
  retryRequest,
  retryableStatus,
  safeRunner,
  staticRequestArgs,
  validateMetadata,
} from "./verify-crate-publication-support.mjs";

async function probeCratesIo({ version, expectedSha256, requireDefault = false, policy, run = defaultRunner, curlCommand = CURL_COMMAND }) {
  return retryRequest(policy, async (timeoutMs) => {
    let output;
    try {
      output = await safeRunner(run, curlCommand, apiRequestArgs(version, timeoutMs), {
        timeoutMs,
        maxBuffer: MAX_API_OUTPUT_BYTES,
      });
    } catch (error) {
      if (error instanceof VerificationError && error.retryable) return { retryable: true };
      throw error;
    }
    const response = parseCurlResponse(output);
    if (response.status === 200) {
      return { retryable: false, metadata: validateMetadata(response.body, version, expectedSha256, requireDefault) };
    }
    if (retryableStatus(response.status)) return { retryable: true };
    fail("crates.io returned HTTP " + response.status);
  }, "crates.io did not provide a stable public version");
}

async function downloadOnce({ version, policy, destination, run, curlCommand }) {
  try {
    fs.rmSync(destination, { force: true });
  } catch {
    fail("could not prepare the temporary crate archive");
  }
  let output;
  try {
    output = await safeRunner(run, curlCommand, staticRequestArgs(version, destination, childTimeoutMs(policy)), {
      timeoutMs: childTimeoutMs(policy),
      maxBuffer: 4096,
    });
  } catch (error) {
    if (error instanceof VerificationError && error.retryable) return { retryable: true };
    throw error;
  }
  const response = parseCurlResponse(output);
  if (retryableStatus(response.status)) return { retryable: true };
  if (response.status !== 200) fail("static crates.io returned HTTP " + response.status);
  return { retryable: false, archive: readStableFile(destination, "downloaded crate archive") };
}

async function downloadCrate({ version, policy, destination, run = defaultRunner, curlCommand = CURL_COMMAND }) {
  const result = await retryRequest(policy, (timeoutMs) => downloadOnce({
    version, policy: { ...policy, childMs: timeoutMs }, destination, run, curlCommand,
  }), "static crates.io archive was not available");
  return result.archive;
}

function makeTempDirectory(parent) {
  try {
    const root = parent ? path.resolve(parent) : os.tmpdir();
    return fs.mkdtempSync(path.join(root, "hardgate-crate-publication-"));
  } catch {
    fail("could not create a temporary verification directory");
  }
}

function cleanTempDirectory(directory) {
  try {
    fs.rmSync(directory, { recursive: true, force: true });
  } catch {
    fail("could not remove the temporary verification directory");
  }
}

function normalizeRequest(request) {
  const expectedPath = request?.expectedPath ?? request?.expected;
  const sourceSha = request?.sourceSha ?? request?.source_sha;
  return {
    expectedPath: assertExpectedPath(expectedPath),
    sourceSha: assertSourceSha(sourceSha),
    version: assertVersion(request?.version),
  };
}

function publicArchive(downloaded, destination) {
  if (Buffer.isBuffer(downloaded)) return { bytes: downloaded, sha256: digest(downloaded) };
  if (downloaded?.bytes) {
    const bytes = Buffer.from(downloaded.bytes);
    return { bytes, sha256: digest(bytes) };
  }
  if (downloaded?.archive) return downloaded.archive;
  if (downloaded?.sha256) return downloaded;
  return readStableFile(destination, "downloaded crate archive");
}

export async function verifyCratePublication(request, operations = {}) {
  const normalized = normalizeRequest(request);
  const policy = operations.policy ?? request.policy ?? verificationPolicy(normalized.version);
  const run = operations.run ?? operations.runner ?? operations.runProcess ?? defaultRunner;
  const tarCommand = operations.tarCommand ?? TAR_COMMAND;
  const curlCommand = operations.curlCommand ?? CURL_COMMAND;
  const expected = readStableFile(normalized.expectedPath, "expected crate archive");
  await readCargoVcsInfo({
    archivePath: normalized.expectedPath,
    version: normalized.version,
    sourceSha: normalized.sourceSha,
    policy,
    run,
    tarCommand,
  });
  const temporaryDirectory = makeTempDirectory(operations.tempDirectory);
  const downloadedPath = path.join(temporaryDirectory, "hardgate-" + normalized.version + ".crate");
  try {
    await (operations.probe ?? probeCratesIo)({
      version: normalized.version,
      expectedSha256: expected.sha256,
      requireDefault: Boolean(request.requireDefault),
      policy,
      run,
      curlCommand,
    });
    const downloaded = await (operations.download ?? downloadCrate)({
      version: normalized.version,
      policy,
      destination: downloadedPath,
      run,
      curlCommand,
    });
    const publicBytes = publicArchive(downloaded, downloadedPath);
    if (publicBytes.sha256 !== expected.sha256 || !publicBytes.bytes?.equals(expected.bytes)) {
      fail("public crates.io archive does not byte-match the expected Cargo archive");
    }
    const expectedAfter = readStableFile(normalized.expectedPath, "expected crate archive");
    if (expectedAfter.sha256 !== expected.sha256 || !expectedAfter.bytes.equals(expected.bytes)) {
      fail("expected crate archive changed during public verification");
    }
    return { version: normalized.version, source_sha: normalized.sourceSha, sha256: expected.sha256 };
  } finally {
    cleanTempDirectory(temporaryDirectory);
  }
}

async function main(argv = process.argv.slice(2)) {
  const values = parseArguments(argv);
  const proof = await verifyCratePublication(values);
  process.stdout.write(JSON.stringify(proof) + "\n");
}

const invokedPath = process.argv[1];
if (invokedPath && import.meta.url === pathToFileURL(invokedPath).href) {
  main().catch(() => {
    process.stderr.write("verify-crate-publication: verification failed\n");
    process.exitCode = 1;
  });
}
