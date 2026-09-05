// Inspect exact npm pack output without rewriting or shelling out to tar.
"use strict";

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";

export const WRAPPER_NAME = "@tech-byte-frontier/hardgate";
export const PLATFORM_PACKAGES = [
  "hardgate-linux-x64",
  "hardgate-linux-x64-musl",
  "hardgate-linux-arm64",
  "hardgate-linux-arm64-musl",
  "hardgate-darwin-x64",
  "hardgate-darwin-arm64",
];
export const MAX_ARCHIVE_BYTES = 64 * 1024 * 1024;
const MAX_TOTAL_ARCHIVE_BYTES = 256 * 1024 * 1024;
const MAX_UNPACKED_ARCHIVE_BYTES = 128 * 1024 * 1024;
const MAX_TOTAL_UNPACKED_BYTES = 512 * 1024 * 1024;
const MAX_ARCHIVE_MEMBERS = 1024;
const MAX_MEMBER_BYTES = 64 * 1024 * 1024;
const MAX_TOTAL_MEMBER_BYTES = 256 * 1024 * 1024;
const MAX_BINARY_BYTES = 256 * 1024 * 1024;
const LIFECYCLE_HOOKS = [
  "preinstall", "install", "postinstall", "prepare", "prepublish", "prepublishOnly", "prepack", "postpack",
];

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

function readBoundedFile(candidate, label, maximum) {
  let fd;
  try {
    fd = fs.openSync(candidate, "r");
    const before = fs.fstatSync(fd);
    if (!before.isFile()) fail(`${label} must be a regular file: ${candidate}`);
    if (before.size > maximum) fail(`${label} exceeds ${maximum} bytes: ${candidate}`);
    const bytes = Buffer.allocUnsafe(before.size);
    let offset = 0;
    while (offset < before.size) {
      const count = fs.readSync(fd, bytes, offset, before.size - offset, offset);
      if (count === 0) fail(`${label} changed while being read: ${candidate}`);
      offset += count;
    }
    const after = fs.fstatSync(fd);
    if (after.size !== before.size || after.mtimeNs !== before.mtimeNs) fail(`${label} changed while being read: ${candidate}`);
    return bytes;
  } finally {
    if (fd !== undefined) fs.closeSync(fd);
  }
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

function tarString(bytes, offset, length) {
  return bytes.subarray(offset, offset + length).toString("utf8").replace(/\0.*$/s, "");
}

function tarNumber(bytes, offset, length) {
  const text = tarString(bytes, offset, length).trim();
  if (!text) return 0;
  const value = Number.parseInt(text, 8);
  if (!Number.isSafeInteger(value) || value < 0) fail(`invalid tar size field: ${JSON.stringify(text)}`);
  return value;
}

function archiveEntries(archiveBytes, archivePath, budget) {
  let bytes;
  try {
    bytes = zlib.gunzipSync(archiveBytes, { maxOutputLength: MAX_UNPACKED_ARCHIVE_BYTES });
  } catch (error) {
    fail(`could not decompress bounded npm archive ${archivePath}: ${error.message}`);
  }
  budget.unpacked += bytes.length;
  if (budget.unpacked > MAX_TOTAL_UNPACKED_BYTES) fail(`npm archives exceed ${MAX_TOTAL_UNPACKED_BYTES} unpacked bytes`);
  const entries = new Map();
  for (let offset = 0; offset + 512 <= bytes.length; ) {
    const header = bytes.subarray(offset, offset + 512);
    if (header.every((value) => value === 0)) break;
    budget.members += 1;
    if (budget.members > MAX_ARCHIVE_MEMBERS) fail(`npm archives exceed ${MAX_ARCHIVE_MEMBERS} members`);
    const name = tarString(header, 0, 100);
    const prefix = tarString(header, 345, 155);
    const member = prefix ? `${prefix}/${name}` : name;
    const size = tarNumber(header, 124, 12);
    if (size > MAX_MEMBER_BYTES) fail(`npm archive member exceeds ${MAX_MEMBER_BYTES} bytes: ${member}`);
    budget.memberBytes += size;
    if (budget.memberBytes > MAX_TOTAL_MEMBER_BYTES) fail(`npm archive members exceed ${MAX_TOTAL_MEMBER_BYTES} bytes: ${archivePath}`);
    const start = offset + 512;
    const end = start + size;
    if (end > bytes.length) fail(`npm archive member exceeds archive length: ${member}`);
    if (member && !entries.has(member)) entries.set(member, Buffer.from(bytes.subarray(start, end)));
    offset = start + Math.ceil(size / 512) * 512;
  }
  return entries;
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

export function hostPlatformPackage() {
  if (process.platform === "darwin") {
    if (process.arch === "x64") return "hardgate-darwin-x64";
    if (process.arch === "arm64") return "hardgate-darwin-arm64";
    return null;
  }
  if (process.platform !== "linux") return null;
  const musl = (() => {
    try {
      const report = process.report?.getReport?.();
      const version = report?.header?.glibcVersionRuntime;
      return !(typeof version === "string" && version.trim().length > 0);
    } catch {
      return true;
    }
  })();
  if (process.arch === "x64") return `hardgate-linux-x64${musl ? "-musl" : ""}`;
  if (process.arch === "arm64") return `hardgate-linux-arm64${musl ? "-musl" : ""}`;
  return null;
}

function readArtifact(archivePath, directory, expectedVersion, budget) {
  const stat = fs.statSync(archivePath);
  if (!stat.isFile()) fail(`archive is not a regular file: ${archivePath}`);
  if (stat.size > MAX_ARCHIVE_BYTES) fail(`archive exceeds ${MAX_ARCHIVE_BYTES} bytes: ${archivePath}`);
  budget.compressed += stat.size;
  if (budget.compressed > MAX_TOTAL_ARCHIVE_BYTES) fail(`npm archives exceed ${MAX_TOTAL_ARCHIVE_BYTES} compressed bytes`);
  const archiveBytes = readBoundedFile(archivePath, "npm archive", MAX_ARCHIVE_BYTES);
  const entries = archiveEntries(archiveBytes, archivePath, budget);
  const manifest = archiveManifest(entries, archivePath);
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
    entries,
    manifest,
  };
}

function dependencyEntries(manifest, field) {
  const value = manifest[field];
  if (value === undefined) return [];
  if (!value || typeof value !== "object" || Array.isArray(value)) fail(`${manifest.name} ${field} must be an object`);
  return Object.entries(value);
}

function validateManifestContract(manifest) {
  for (const field of ["dependencies", "devDependencies", "peerDependencies"]) {
    if (Object.hasOwn(manifest, field)) fail(`${manifest.name} must not declare ${field}`);
  }
  if (Object.hasOwn(manifest, "bundledDependencies") || Object.hasOwn(manifest, "bundleDependencies")) {
    fail(`${manifest.name} must not declare bundled dependencies`);
  }
  const scripts = manifest.scripts;
  if (scripts !== undefined && (!scripts || typeof scripts !== "object" || Array.isArray(scripts))) fail(`${manifest.name} scripts must be an object`);
  for (const hook of LIFECYCLE_HOOKS) {
    if (Object.hasOwn(scripts ?? {}, hook)) fail(`${manifest.name} must not declare npm lifecycle hook ${hook}`);
  }
  for (const [name, value] of dependencyEntries(manifest, "optionalDependencies")) {
    if (typeof value !== "string" || /^(?:file|link|workspace|npm|http|https|git|github|ssh):/i.test(value)) {
      fail(`${manifest.name} optionalDependency ${name} must be a registry version`);
    }
  }
}

function validateWrapper(wrapper, expectedVersion) {
  if (!wrapper) fail(`--packages-dir is missing ${WRAPPER_NAME}@${expectedVersion}.tgz`);
  const launcherBytes = wrapper.entries.get("package/bin/hardgate.js");
  if (!launcherBytes) fail(`${WRAPPER_NAME}@${expectedVersion}.tgz is missing package/bin/hardgate.js`);
  const optional = wrapper.manifest.optionalDependencies ?? {};
  const expectedNames = [...PLATFORM_PACKAGES].sort();
  if (JSON.stringify(Object.keys(optional).sort()) !== JSON.stringify(expectedNames)) {
    fail(`${WRAPPER_NAME} optionalDependencies do not match the six supported platform packages`);
  }
  for (const name of PLATFORM_PACKAGES) {
    if (optional[name] !== expectedVersion) fail(`${WRAPPER_NAME} optionalDependencies[${name}] must be ${expectedVersion}`);
  }
  return launcherBytes;
}

function validateHost({ hostArtifact, host, expectedBytes, expectedHash, expectedVersion }) {
  if (!hostArtifact) fail(`--packages-dir is missing host optional dependency ${host}@${expectedVersion}`);
  const nativeBytes = hostArtifact.entries.get("package/bin/hardgate");
  if (!nativeBytes) fail(`${host}@${expectedVersion}.tgz is missing package/bin/hardgate`);
  if (!nativeBytes.equals(expectedBytes)) fail(`${host} archive binary bytes do not match --binary (expected sha256 ${expectedHash})`);
  if ((hostArtifact.manifest.os ?? []).length === 0 || (hostArtifact.manifest.cpu ?? []).length === 0) {
    fail(`${host} manifest must declare os and cpu constraints`);
  }
  if (Object.hasOwn(hostArtifact.manifest, "optionalDependencies")) {
    fail(`${host} must not declare optionalDependencies`);
  }
  validateManifestContract(hostArtifact.manifest);
}

export function inspectPackedArtifacts(packagesDir, version, binaryPath) {
  const expectedVersion = requiredString(version, "--version");
  const expectedBinary = absoluteFile(binaryPath, "--binary", true);
  const expectedBytes = readBoundedFile(expectedBinary, "--binary", MAX_BINARY_BYTES);
  const expectedHash = sha256(expectedBytes);
  const files = packageArchiveFiles(packagesDir);
  const artifacts = new Map();
  const budget = { compressed: 0, members: 0, memberBytes: 0, unpacked: 0 };
  for (const archivePath of files.files) {
    const artifact = readArtifact(archivePath, files.directory, expectedVersion, budget);
    if (artifacts.has(artifact.name)) fail(`duplicate archive for npm package ${artifact.name}`);
    artifacts.set(artifact.name, artifact);
  }
  const wrapperLauncherBytes = validateWrapper(artifacts.get(WRAPPER_NAME), expectedVersion);
  const host = hostPlatformPackage();
  if (!host) fail(`unsupported consumer platform ${process.platform}/${process.arch}`);
  validateHost({ hostArtifact: artifacts.get(host), host, expectedBytes, expectedHash, expectedVersion });
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
