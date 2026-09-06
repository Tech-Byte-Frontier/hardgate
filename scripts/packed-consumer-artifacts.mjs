// Inspect exact npm pack output without rewriting or shelling out to tar.
"use strict";

import { detectHost, hostNativePackage } from "./native-channel-support.mjs";

import { PLATFORM_CONTRACT as RELEASE_CONTRACT } from "./release-platforms.mjs";

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

import { archiveEntries, MAX_ARCHIVE_BYTES, MAX_BINARY_BYTES, readBoundedFile } from "./packed-consumer-tar.mjs";
import { validateHost, validateManifestContract, validatePlatformArtifact, validateWrapper } from "./packed-consumer-contract.mjs";

export const WRAPPER_NAME = "@tech-byte-frontier/hardgate";
// Keep this contract aligned with scripts/verify-npm-publication.mjs, which is
// the release verifier's authoritative platform map.
const PLATFORM_CONTRACT = RELEASE_CONTRACT;
const PLATFORM_PACKAGES = PLATFORM_CONTRACT.map(({ name }) => name);
const MAX_TOTAL_ARCHIVE_BYTES = 256 * 1024 * 1024;

function fail(message) {
  throw new Error(message);
}

function requiredString(value, label) {
  if (typeof value !== "string" || value.trim().length === 0) fail(`${label} is required`);
  return value.trim();
}

function absoluteFile(candidate, label, executable = false) {
  const resolved = path.resolve(candidate);
  let stat;
  try {
    stat = fs.statSync(resolved);
  } catch (error) {
    fail(`${label} does not exist: ${resolved} (${error.message})`);
  }
  if (!stat.isFile()) fail(`${label} must be a regular file: ${resolved}`);
  if (executable && (stat.mode & 0o111) === 0) fail(`${label} must be executable: ${resolved}`);
  return resolved;
}

function sha256(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex");
}

function sha1(bytes) {
  return crypto.createHash("sha1").update(bytes).digest("hex");
}

function integrity(bytes) {
  return `sha512-${crypto.createHash("sha512").update(bytes).digest("base64")}`;
}

function archiveManifest(entries, archivePath) {
  const manifestBytes = entries.get("package/package.json");
  if (!manifestBytes) fail(`npm archive is missing package/package.json: ${archivePath}`);
  try {
    const manifest = JSON.parse(manifestBytes.toString("utf8"));
    if (!manifest || typeof manifest !== "object" || Array.isArray(manifest)) throw new Error("manifest is not an object");
    return manifest;
  } catch (error) {
    fail(`npm archive manifest is invalid: ${archivePath} (${error.message})`);
  }
}

function packageArchiveFiles(packagesDir) {
  const directory = path.resolve(requiredString(packagesDir, "--packages-dir"));
  let stat;
  try {
    stat = fs.statSync(directory);
  } catch (error) {
    fail(`--packages-dir does not exist: ${directory} (${error.message})`);
  }
  if (!stat.isDirectory()) fail(`--packages-dir must be a directory: ${directory}`);
  const files = fs.readdirSync(directory, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(".tgz"))
    .map((entry) => path.join(directory, entry.name));
  if (files.length === 0) fail(`--packages-dir has no .tgz archives: ${directory}`);
  let totalBytes = 0;
  for (const file of files) {
    const stat = fs.statSync(file);
    if (!stat.isFile()) fail(`archive is not a regular file: ${file}`);
    if (stat.size > MAX_ARCHIVE_BYTES) fail(`archive exceeds ${MAX_ARCHIVE_BYTES} bytes: ${file}`);
    totalBytes += stat.size;
  }
  if (totalBytes > MAX_TOTAL_ARCHIVE_BYTES) fail(`npm archives exceed ${MAX_TOTAL_ARCHIVE_BYTES} compressed bytes`);
  return { directory, files };
}

function hostPlatformPackage() {
  return hostNativePackage(detectHost());
}

function readArtifact(archivePath, directory, expectedVersion, budget) {
  const stat = fs.statSync(archivePath);
  if (!stat.isFile()) fail(`archive is not a regular file: ${archivePath}`);
  if (stat.size > MAX_ARCHIVE_BYTES) fail(`archive exceeds ${MAX_ARCHIVE_BYTES} bytes: ${archivePath}`);
  budget.compressed += stat.size;
  if (budget.compressed > MAX_TOTAL_ARCHIVE_BYTES) fail(`npm archives exceed ${MAX_TOTAL_ARCHIVE_BYTES} compressed bytes`);
  const archiveBytes = readBoundedFile(archivePath, "npm archive", MAX_ARCHIVE_BYTES);
  const archive = archiveEntries(archiveBytes, archivePath, budget);
  const manifest = archiveManifest(archive.entries, archivePath);
  const name = requiredString(manifest.name, `archive ${archivePath} manifest.name`);
  if (name !== WRAPPER_NAME && !PLATFORM_PACKAGES.includes(name)) fail(`unsupported npm package in --packages-dir: ${name}`);
  if (manifest.version !== expectedVersion) fail(`${name} archive version ${manifest.version} does not match --version ${expectedVersion}`);
  validateManifestContract(manifest);
  const resolved = fs.realpathSync(archivePath);
  const relative = path.relative(directory, resolved);
  if (relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) fail(`archive escapes --packages-dir: ${archivePath}`);
  return {
    name,
    version: expectedVersion,
    archivePath: resolved,
    archiveBytes,
    archiveSha256: sha256(archiveBytes),
    shasum: sha1(archiveBytes),
    integrity: integrity(archiveBytes),
    entries: archive.entries,
    entryModes: archive.entryModes,
    manifest,
  };
}

function readArtifactSet(files, expectedVersion) {
  const artifacts = new Map();
  const budget = { compressed: 0, members: 0, memberBytes: 0, unpacked: 0 };
  for (const archivePath of files.files) {
    const artifact = readArtifact(archivePath, files.directory, expectedVersion, budget);
    if (artifacts.has(artifact.name)) fail(`duplicate archive for npm package ${artifact.name}`);
    artifacts.set(artifact.name, artifact);
  }
  return artifacts;
}

function requireExactPackageSet(artifacts, expectedVersion) {
  const expectedNames = new Set([WRAPPER_NAME, ...PLATFORM_PACKAGES]);
  const missingNames = [...expectedNames].filter((name) => !artifacts.has(name));
  const extraNames = [...artifacts.keys()].filter((name) => !expectedNames.has(name));
  if (missingNames.length === 0 && extraNames.length === 0 && artifacts.size === expectedNames.size) return;
  const missingLabel = missingNames.map((name) => `${name}@${expectedVersion}`).join(", ") || "none";
  fail(`--packages-dir must contain exactly the wrapper and the Linux x64 GNU platform archive (missing: ${missingLabel}; extra: ${extraNames.join(", ") || "none"})`);
}

function validateNonHostPlatforms(artifacts, host, expectedVersion) {
  for (const descriptor of PLATFORM_CONTRACT) {
    if (descriptor.name !== host) validatePlatformArtifact({ artifact: artifacts.get(descriptor.name), descriptor, expectedVersion });
  }
}

export function inspectPackedArtifacts(packagesDir, version, binaryPath) {
  const expectedVersion = requiredString(version, "--version");
  const expectedBinary = absoluteFile(binaryPath, "--binary", true);
  const expectedBytes = readBoundedFile(expectedBinary, "--binary", MAX_BINARY_BYTES);
  const expectedHash = sha256(expectedBytes);
  const files = packageArchiveFiles(packagesDir);
  const artifacts = readArtifactSet(files, expectedVersion);
  const wrapperLauncherBytes = validateWrapper(artifacts.get(WRAPPER_NAME), expectedVersion, WRAPPER_NAME, PLATFORM_PACKAGES);
  const host = hostPlatformPackage();
  if (!host) fail(`unsupported consumer platform ${process.platform}/${process.arch}`);
  validateHost({
    hostArtifact: artifacts.get(host),
    host,
    descriptor: PLATFORM_CONTRACT.find((candidate) => candidate.name === host),
    expectedBytes,
    expectedHash,
    expectedVersion,
  });
  requireExactPackageSet(artifacts, expectedVersion);
  validateNonHostPlatforms(artifacts, host, expectedVersion);
  return { expectedBinary, expectedBytes, expectedHash, expectedVersion, host, wrapperLauncherBytes, artifacts };
}

export function snapshotArchiveFiles(artifacts) {
  return [...artifacts.values()].map((artifact) => {
    const stat = fs.statSync(artifact.archivePath);
    return {
      path: artifact.archivePath,
      realpath: fs.realpathSync(artifact.archivePath),
      size: stat.size,
      mode: stat.mode & 0o7777,
      sha256: artifact.archiveSha256,
    };
  });
}

export function verifyArchiveSnapshot(snapshot) {
  const failures = [];
  for (const expected of snapshot) {
    try {
      const actualPath = fs.realpathSync(expected.path);
      const stat = fs.statSync(expected.path);
      if (actualPath !== expected.realpath) throw new Error("real path changed");
      if (!stat.isFile() || stat.size !== expected.size || (stat.mode & 0o7777) !== expected.mode) throw new Error("file metadata changed");
      const actualHash = sha256(readBoundedFile(expected.path, "packed archive", MAX_ARCHIVE_BYTES));
      if (actualHash !== expected.sha256) throw new Error(`sha256 changed to ${actualHash}`);
    } catch (error) {
      failures.push(new Error(`${expected.path}: ${error.message}`, { cause: error }));
    }
  }
  if (failures.length > 0) throw new AggregateError(failures, "packed archive snapshot changed");
}
