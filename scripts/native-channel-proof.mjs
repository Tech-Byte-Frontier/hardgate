// Strict proof schema and durable atomic proof output for native npm checks.
"use strict";

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import {
  PROOF_VERSION,
  assertSourceSha,
  assertVersion,
  fail,
  packageDescriptor,
  stableExecutablePath,
  assertStableExecutablePath,
} from "./native-channel-support.mjs";

const HASH = /^[0-9a-f]{64}$/;

function assertMode(mode) {
  if (mode !== "exact" && mode !== "default") fail(`--mode must be exact or default, got ${mode || "<missing>"}`);
  return mode;
}

function assertHash(value, label) {
  if (typeof value !== "string" || !HASH.test(value)) fail(`${label} must be 64 lowercase hexadecimal characters`);
  return value;
}

function assertProofConsumer(value, label) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) fail(`${label} must be an object`);
  const keys = Object.keys(value).sort();
  if (keys.join("\n") !== ["executable", "sha256"].join("\n")) fail(`${label} has unexpected fields`);
  return {
    executable: assertStableExecutablePath(value.executable, `${label}.executable`),
    sha256: assertHash(value.sha256, `${label}.sha256`),
  };
}

function assertProofShape(proof) {
  if (proof === null || typeof proof !== "object" || Array.isArray(proof)) fail("proof must be an object");
  const keys = Object.keys(proof).sort();
  const expected = ["archive", "consumer", "mode", "package", "schema_version", "source_sha", "version"];
  const withWrapper = [...expected, "wrapper"].sort();
  if (keys.join("\n") !== expected.sort().join("\n") && keys.join("\n") !== withWrapper.join("\n")) {
    fail("proof has unexpected fields");
  }
  if (proof.schema_version !== PROOF_VERSION) fail("proof.schema_version is unsupported");
}

function proofArchive(packageName, archive) {
  if (archive === null || typeof archive !== "object" || Array.isArray(archive)) fail("proof.archive must be an object");
  const archiveKeys = Object.keys(archive).sort();
  if (archiveKeys.join("\n") !== "name\nsha256") fail("proof.archive has unexpected fields");
  const expectedName = `${packageName}.tar.gz`;
  if (archive.name !== expectedName) fail(`proof.archive.name must be ${expectedName}`);
  return { name: expectedName, sha256: assertHash(archive.sha256, "proof.archive.sha256") };
}

function proofConsumer(packageName, value, label) {
  const consumer = assertProofConsumer(value, label);
  if (consumer.executable !== stableExecutablePath(packageName)) {
    fail(`${label}.executable does not identify the requested package`);
  }
  return consumer;
}

export function validateProof(proof) {
  assertProofShape(proof);
  const packageName = packageDescriptor(proof.package).name;
  const version = assertVersion(proof.version);
  const sourceSha = assertSourceSha(proof.source_sha, "proof.source_sha");
  const mode = assertMode(proof.mode);
  const archive = proofArchive(packageName, proof.archive);
  const consumer = proofConsumer(packageName, proof.consumer, "proof.consumer");
  const result = {
    schema_version: PROOF_VERSION,
    version,
    source_sha: sourceSha,
    mode,
    package: packageName,
    archive,
    consumer,
  };
  if (Object.hasOwn(proof, "wrapper")) {
    if (packageName !== "hardgate-linux-x64") fail("proof.wrapper is only valid for the canonical Linux x64 GNU package");
    result.wrapper = assertProofConsumer(proof.wrapper, "proof.wrapper");
    if (result.wrapper.executable !== consumer.executable) fail("proof.wrapper.executable does not identify the resolved native package");
  } else if (packageName === "hardgate-linux-x64") {
    fail("proof.wrapper is required for the canonical Linux x64 GNU package");
  }
  return result;
}

function targetSnapshot(target) {
  try {
    const stats = fs.lstatSync(target);
    if (stats.isSymbolicLink() || !stats.isFile()) fail("--output must identify a regular file");
    return { exists: true, bytes: fs.readFileSync(target), mode: stats.mode & 0o7777 };
  } catch (error) {
    if (error.code === "ENOENT") return { exists: false };
    throw error;
  }
}

function removeRegularTarget(target) {
  try {
    const stats = fs.lstatSync(target);
    if (!stats.isFile() && !stats.isSymbolicLink()) throw new Error("--output target changed type during atomic write");
    fs.unlinkSync(target);
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
}

function syncDirectory(directory) {
  try {
    const descriptor = fs.openSync(directory, "r");
    try {
      fs.fsyncSync(descriptor);
    } finally {
      fs.closeSync(descriptor);
    }
  } catch (error) {
    if (!["EINVAL", "ENOTSUP", "EISDIR"].includes(error.code)) throw error;
  }
}

function writeTemporary(temporary, bytes) {
  const descriptor = fs.openSync(temporary, "wx", 0o600);
  try {
    fs.writeFileSync(descriptor, bytes);
    fs.fsyncSync(descriptor);
  } finally {
    fs.closeSync(descriptor);
  }
}

function installReplacement(state) {
  if (state.previous.exists) {
    fs.renameSync(state.target, state.backup);
    state.backupMoved = true;
  }
  fs.renameSync(state.temporary, state.target);
  state.replacementMoved = true;
  fs.chmodSync(state.target, 0o600);
  syncDirectory(state.directory);
}

function restoreSnapshot(state) {
  const restore = path.join(state.directory, `.${path.basename(state.target)}.${process.pid}.${crypto.randomBytes(12).toString("hex")}.restore`);
  let descriptor;
  try {
    descriptor = fs.openSync(restore, "wx", state.previous.mode & 0o7777);
    fs.writeFileSync(descriptor, state.previous.bytes);
    fs.fsyncSync(descriptor);
    fs.closeSync(descriptor);
    descriptor = undefined;
    fs.renameSync(restore, state.target);
  } finally {
    if (descriptor !== undefined) fs.closeSync(descriptor);
    try {
      fs.unlinkSync(restore);
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
  }
}

function restorePrevious(state) {
  if (state.backupMoved) {
    fs.renameSync(state.backup, state.target);
    state.backupMoved = false;
  } else if (state.previous.exists) {
    restoreSnapshot(state);
  }
}

function removeAtomicArtifacts(state) {
  for (const pathToRemove of [state.temporary, state.backup]) {
    try {
      fs.unlinkSync(pathToRemove);
    } catch (error) {
      if (error.code !== "ENOENT") return;
    }
  }
}

function rollbackProof(state) {
  try {
    if (state.replacementMoved) removeRegularTarget(state.target);
    restorePrevious(state);
    syncDirectory(state.directory);
  } catch {
    // Preserve the original write error while making the best effort to
    // remove replacement paths and restore the prior target above.
  }
  removeAtomicArtifacts(state);
}

function finalizeReplacement(state) {
  if (!state.backupMoved) return;
  fs.unlinkSync(state.backup);
  state.backupMoved = false;
  syncDirectory(state.directory);
}

export function writeProofAtomic(output, proof) {
  const target = path.resolve(output);
  const checked = validateProof(proof);
  const directory = path.dirname(target);
  fs.mkdirSync(directory, { recursive: true, mode: 0o700 });
  const state = {
    target,
    directory,
    previous: targetSnapshot(target),
    temporary: path.join(directory, `.${path.basename(target)}.${process.pid}.${crypto.randomBytes(12).toString("hex")}.tmp`),
    backup: path.join(directory, `.${path.basename(target)}.${process.pid}.${crypto.randomBytes(12).toString("hex")}.bak`),
    replacementMoved: false,
    backupMoved: false,
  };
  const bytes = Buffer.from(`${JSON.stringify(checked, null, 2)}\n`, "utf8");
  try {
    writeTemporary(state.temporary, bytes);
    installReplacement(state);
    finalizeReplacement(state);
  } catch (error) {
    rollbackProof(state);
    throw error;
  }
  return checked;
}
