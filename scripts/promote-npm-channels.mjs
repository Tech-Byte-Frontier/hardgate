#!/usr/bin/env node
// Promote the seven npm latest channels from a verified release receipt.
//
// This command only moves npm dist-tags. It does not publish package bytes,
// perform an OIDC exchange, or mark a receipt's default-consumer state. The
// receipt remains the durable source of version, source identity, and archive
// digests.
"use strict";

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  CHANNELS,
  REQUIRED_CHANNELS,
  RECEIPT_STATES,
  readReceipt,
  recordFailure,
  recordTransition,
  writeReceiptAtomicSync,
} from "./release-receipt.mjs";
import { verificationPolicy } from "./npm-verification-policy.mjs";
import { runReleaseProcess } from "./release-process.mjs";
import {
  NPM_CHANNELS,
  NPM_REGISTRY,
  NpmPromotionError,
  probeNpmLatest,
  promoteNpmChannel,
} from "./npm-channel-promotion.mjs";

const MAX_ARCHIVE_BYTES = 1024 * 1024 * 1024;
const REQUIRED_NPM_RECEIPT_STATE = "exact_consumer_verified";

const HASH_CHUNK_BYTES = 64 * 1024;
const READ_FLAGS = fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW ?? 0);
const FAILURE_MESSAGES = Object.freeze({
  npm_auth_missing: "npm promotion credential is missing",
  npm_latest_newer: "npm latest channel is newer than the requested release",
  npm_latest_identity: "npm latest metadata identity does not match the requested release",
  npm_latest_metadata: "npm latest metadata is invalid",
  npm_latest_http: "npm latest probe returned a fatal HTTP status",
  npm_latest_probe: "npm latest probe failed",
  npm_latest_response: "npm latest probe returned an invalid response",
  npm_immutable_failed: "immutable npm payload verification failed",
  npm_mutation_failed: "npm latest tag mutation failed",
  npm_readback_failed: "npm latest tag readback did not identify the requested release",
  npm_default_mismatch: "npm latest selector verification failed",
  npm_timeout: "npm promotion operation deadline exhausted",
  npm_child_failed: "npm promotion subprocess failed",
  npm_channel_failed: "npm channel promotion failed",
  npm_promotion_failed: "npm channel promotion failed",
});
const FAILURE_CODES = new Set(Object.keys(FAILURE_MESSAGES));

function fail(code, message, cause) {
  return new NpmPromotionError(code, message, cause);
}

function assertRegularDirectory(directory, label) {
  let stats;
  try {
    stats = fs.lstatSync(directory);
  } catch (error) {
    throw fail("npm_invalid_request", `${label} is not an existing directory`, error);
  }
  if (stats.isSymbolicLink() || !stats.isDirectory()) {
    throw fail("npm_invalid_request", `${label} is not an existing directory`);
  }
  return path.resolve(directory);
}

function archivePath(distDir, archiveName) {
  // Receipt validation already disallows path separators. Keep this check at
  // the filesystem boundary so this caller remains safe if that schema grows.
  if (path.basename(archiveName) !== archiveName) throw fail("npm_dist_integrity", "release archive name is unsafe");
  return path.join(distDir, archiveName);
}

function archiveStat(file) {
  let stats;
  try {
    stats = fs.lstatSync(file);
  } catch (error) {
    throw fail("npm_dist_integrity", "a receipt archive is missing", error);
  }
  if (stats.isSymbolicLink() || !stats.isFile()) throw fail("npm_dist_integrity", "a receipt archive is not a regular file");
  if (!Number.isSafeInteger(stats.size) || stats.size > MAX_ARCHIVE_BYTES) {
    throw fail("npm_dist_integrity", "a receipt archive exceeds the bounded hash size");
  }
}

function archiveDescriptor(file, label) {
  archiveStat(file);
  let descriptor;
  try {
    descriptor = fs.openSync(file, READ_FLAGS);
    const opened = fs.fstatSync(descriptor);
    if (!opened.isFile() || opened.size > MAX_ARCHIVE_BYTES) {
      fs.closeSync(descriptor);
      descriptor = undefined;
      throw fail("npm_dist_integrity", "a receipt archive exceeds the bounded hash size");
    }
    return descriptor;
  } catch (error) {
    if (descriptor !== undefined) fs.closeSync(descriptor);
    if (error instanceof NpmPromotionError) throw error;
    throw fail("npm_dist_integrity", `release archive ${label} could not be hashed`, error);
  }
}

function hashDescriptor(descriptor) {
  const digest = crypto.createHash("sha256");
  const block = Buffer.allocUnsafe(HASH_CHUNK_BYTES);
  let total = 0;
  while (total <= MAX_ARCHIVE_BYTES) {
    const length = fs.readSync(descriptor, block, 0, block.length, null);
    if (!length) return digest.digest("hex");
    total += length;
    if (total > MAX_ARCHIVE_BYTES) throw fail("npm_dist_integrity", "a receipt archive exceeds the bounded hash size");
    digest.update(block.subarray(0, length));
  }
  throw fail("npm_dist_integrity", "a receipt archive exceeds the bounded hash size");
}

function hashArchive(file, label) {
  const descriptor = archiveDescriptor(file, label);
  try {
    return hashDescriptor(descriptor);
  } catch (error) {
    if (error instanceof NpmPromotionError) throw error;
    throw fail("npm_dist_integrity", `release archive ${label} could not be hashed`, error);
  } finally {
    fs.closeSync(descriptor);
  }
}

/** Validate every receipt archive before any public probe or npm mutation. */
function verifyDistributionDigests(receipt, distDir) {
  const directory = assertRegularDirectory(path.resolve(distDir), "dist");
  for (const archive of receipt.identity.archives) {
    const actual = hashArchive(archivePath(directory, archive.name), archive.name);
    if (actual !== archive.sha256) {
      throw fail("npm_dist_integrity", "release archive digest does not match the receipt");
    }
  }
  return directory;
}

function stateAtLeast(state, required) {
  return RECEIPT_STATES.indexOf(state) >= RECEIPT_STATES.indexOf(required);
}

function assertNpmPrerequisites(receipt) {
  for (const channel of REQUIRED_CHANNELS) {
    if (!stateAtLeast(receipt.channels[channel].state, REQUIRED_NPM_RECEIPT_STATE)) {
      throw fail("npm_prerequisite", "all release channels require exact consumer verification before npm promotion");
    }
  }
}

function transitionEvidence(receipt) {
  return {
    version: receipt.identity.version,
    source_sha: receipt.identity.source_sha,
    archives: receipt.identity.archives.map((archive) => ({ ...archive })),
  };
}

function failureDetails(error) {
  const code = FAILURE_CODES.has(error?.code) ? error.code : "npm_promotion_failed";
  return { code, message: FAILURE_MESSAGES[code] };
}

function publicFailure(error, details) {
  if (error instanceof NpmPromotionError && FAILURE_CODES.has(error.code)) return error;
  return fail(details.code, details.message, error);
}

function persistFailure(receiptPath, receipt, channel, error) {
  const details = failureDetails(error);
  recordFailure(receipt, { channel, code: details.code, message: details.message });
  writeReceiptAtomicSync(receiptPath, receipt, receipt.identity);
  return details;
}

function parseArguments(argv) {
  const options = Object.create(null);
  const accepted = new Set(["--receipt", "--dist"]);
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--help" || argument === "-h") return { help: true };
    if (!accepted.has(argument)) throw fail("npm_invalid_request", "usage: --receipt FILE --dist DIR");
    if (Object.hasOwn(options, argument)) throw fail("npm_invalid_request", "each option may be specified only once");
    const value = argv[index + 1];
    if (!value || value.startsWith("-")) throw fail("npm_invalid_request", `${argument} requires a value`);
    options[argument] = value;
    index += 1;
  }
  if (!options["--receipt"] || !options["--dist"]) throw fail("npm_invalid_request", "usage: --receipt FILE --dist DIR");
  return options;
}

const HELP = "Usage: node scripts/promote-npm-channels.mjs --receipt FILE --dist DIR";

/**
 * Promote each platform package and then the wrapper. The supplied receipt is
 * re-read once, gated before any network operation, and saved after each
 * successful channel and each channel-scoped failure.
 */
export async function promoteNpmChannels({
  receiptPath,
  distDir,
  sourceCwd = process.cwd(),
  env = process.env,
  policy,
  runProcess = runReleaseProcess,
  probeLatest = probeNpmLatest,
  verifyImmutable,
  writeReceipt = writeReceiptAtomicSync,
}) {
  if (typeof receiptPath !== "string" || receiptPath.length === 0) throw fail("npm_invalid_request", "receipt path is required");
  const receipt = readReceipt(receiptPath);
  assertNpmPrerequisites(receipt);
  const directory = verifyDistributionDigests(receipt, distDir);
  const source = assertRegularDirectory(path.resolve(sourceCwd), "source");
  const sharedPolicy = policy ?? verificationPolicy(receipt.identity.version, env);
  const results = [];

  for (const channel of NPM_CHANNELS) {
    const stateBefore = receipt.channels[channel].state;
    let operation;
    try {
      operation = await promoteNpmChannel({
        name: channel,
        version: receipt.identity.version,
        exactConsumerVerified: stateAtLeast(stateBefore, REQUIRED_NPM_RECEIPT_STATE),
        distDir: directory,
        sourceCwd: source,
        policy: sharedPolicy,
        env,
        runProcess,
        probeLatest,
        verifyImmutable,
      });
      // A successful latest readback records only the durable promoted state.
      // The native consumer matrix owns default_consumer_verified later.
      if (stateBefore === REQUIRED_NPM_RECEIPT_STATE) {
        recordTransition(receipt, {
          channel,
          from: REQUIRED_NPM_RECEIPT_STATE,
          to: "promoted",
          evidence: transitionEvidence(receipt),
        });
        await writeReceipt(receiptPath, receipt, receipt.identity);
      }
      results.push({ channel, publication: operation.publication, state: receipt.channels[channel].state });
    } catch (error) {
      const details = failureDetails(error);
      const safeError = publicFailure(error, details);
      try {
        persistFailure(receiptPath, receipt, channel, safeError);
      } catch (persistError) {
        throw fail("npm_promotion_failed", "npm channel failure could not be persisted", safeError);
      }
      throw safeError;
    }
  }
  return { version: receipt.identity.version, registry: NPM_REGISTRY, results, receipt };
}

export async function run(argv = process.argv.slice(2), dependencies = {}) {
  const options = parseArguments(argv);
  if (options.help) {
    console.log(HELP);
    return { help: true };
  }
  return promoteNpmChannels({
    receiptPath: options["--receipt"],
    distDir: options["--dist"],
    ...dependencies,
  });
}

function main() {
  run().then((result) => {
    if (!result?.help) {
      console.log(JSON.stringify({
        version: result.version,
        registry: result.registry,
        results: result.results,
      }));
    }
  }).catch((error) => {
    // All production child errors are wrapped with constant messages before
    // reaching here; never print subprocess stdout/stderr or error causes.
    process.stderr.write(`${error instanceof NpmPromotionError ? error.message : "npm channel promotion failed"}\n`);
    process.exitCode = 1;
  });
}

const invokedDirectly = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedDirectly) main();
