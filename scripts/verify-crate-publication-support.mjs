"use strict";

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { childTimeoutMs, remainingMs } from "./npm-verification-policy.mjs";
import { runReleaseProcess } from "./release-process.mjs";
import { compareReleaseTags } from "./release-order.mjs";
import {
  validateDefaultMetadata as validateDefaultMetadataImpl,
  validateMetadata as validateMetadataImpl,
} from "./verify-crate-publication-metadata.mjs";

const CRATE_NAME = "hardgate";
const PROJECT_URL = "https://github.com/Tech-Byte-Frontier/hardgate";
const USER_AGENT = "hardgate-release (" + PROJECT_URL + ")";
const API_ROOT = "https://crates.io/api/v1/crates/hardgate";
const STATIC_ROOT = "https://static.crates.io/crates/hardgate";
export const TAR_COMMAND = "/usr/bin/tar";
export const CURL_COMMAND = "/usr/bin/curl";
const MAX_ARCHIVE_BYTES = 64 * 1024 * 1024;
const MAX_TAR_OUTPUT_BYTES = 1024 * 1024;
export const MAX_API_OUTPUT_BYTES = 4 * 1024 * 1024;
const SOURCE_SHA_PATTERN = /^[0-9a-f]{40}$/;

export class VerificationError extends Error {
  constructor(message, { retryable = false } = {}) {
    super(message);
    this.name = "VerificationError";
    this.retryable = retryable;
  }
}

export function fail(message, options) {
  throw new VerificationError("crate publication verification: " + message, options);
}

export function assertVersion(version) {
  if (typeof version !== "string" || version.length === 0 || version.length > 128) {
    fail("version must be a strict SemVer value");
  }
  try {
    compareReleaseTags("v" + version, "v" + version);
  } catch {
    fail("version must be a strict SemVer value");
  }
  return version;
}

export function assertSourceSha(sourceSha) {
  if (typeof sourceSha !== "string" || !SOURCE_SHA_PATTERN.test(sourceSha)) {
    fail("source-sha must be exactly 40 lowercase hexadecimal characters");
  }
  return sourceSha;
}

export function assertExpectedPath(expectedPath) {
  if (typeof expectedPath !== "string" || expectedPath.length === 0 || expectedPath.includes("\0")) {
    fail("expected archive path is required");
  }
  return path.resolve(expectedPath);
}

export function digest(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex");
}

function statSignature(stat) {
  const mtime = stat.mtimeNs ?? BigInt(Math.trunc(stat.mtimeMs * 1_000_000));
  const ctime = stat.ctimeNs ?? BigInt(Math.trunc(stat.ctimeMs * 1_000_000));
  return [stat.dev, stat.ino, stat.mode, stat.size, mtime, ctime].join(":");
}

function assertRegularStat(stat, description) {
  if (!stat.isFile() || stat.size < 0 || !Number.isSafeInteger(stat.size) || stat.size > MAX_ARCHIVE_BYTES) {
    fail(description + " must be a regular file no larger than 64 MiB");
  }
}

function pathStat(filePath, description) {
  try {
    const stat = fs.lstatSync(filePath, { bigint: false });
    assertRegularStat(stat, description);
    return stat;
  } catch (error) {
    if (error instanceof VerificationError) throw error;
    fail(description + " is unavailable");
  }
}

function openFile(filePath, description) {
  try {
    return fs.openSync(filePath, fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW ?? 0));
  } catch {
    fail(description + " could not be opened safely");
  }
}

function fdStat(fd, description) {
  try {
    const stat = fs.fstatSync(fd, { bigint: false });
    assertRegularStat(stat, description);
    return stat;
  } catch (error) {
    if (error instanceof VerificationError) throw error;
    fail(description + " could not be inspected safely");
  }
}

function readFd(fd, size, description) {
  const bytes = Buffer.allocUnsafe(size);
  let offset = 0;
  while (offset < size) {
    let count;
    try {
      count = fs.readSync(fd, bytes, offset, size - offset, null);
    } catch {
      fail("could not read " + description);
    }
    if (count === 0) fail(description + " changed while it was being read");
    offset += count;
  }
  return bytes;
}

function closeFile(fd) {
  try {
    fs.closeSync(fd);
  } catch {
    // Preserve the verification result; close cannot make bytes more trusted.
  }
}
export function readStableFile(filePath, description) {
  const initial = pathStat(filePath, description);
  const fd = openFile(filePath, description);
  try {
    const before = fdStat(fd, description);
    if (statSignature(before) !== statSignature(initial)) fail(description + " changed before it was read");
    const bytes = readFd(fd, before.size, description);
    const after = fdStat(fd, description);
    if (statSignature(before) !== statSignature(after)) fail(description + " changed while it was being read");
    const finalPath = pathStat(filePath, description);
    if (statSignature(before) !== statSignature(finalPath)) fail(description + " was replaced while it was being read");
    return { bytes, sha256: digest(bytes), stat: after };
  } finally {
    closeFile(fd);
  }
}

function publicEnvironment() {
  const allowlist = ["PATH", "TMPDIR", "TMP", "TEMP", "LANG", "LC_ALL", "LC_CTYPE", "TZ"];
  const environment = {};
  for (const name of allowlist) {
    if (typeof process.env[name] === "string") environment[name] = process.env[name];
  }
  return environment;
}

function normalizeOutput(output) {
  if (Buffer.isBuffer(output)) return output.toString("utf8");
  if (typeof output === "string") return output;
  fail("verification subprocess returned invalid output");
}

export function parseCurlResponse(output) {
  const text = normalizeOutput(output);
  const marker = text.lastIndexOf("\n");
  if (marker < 0) fail("verification subprocess returned no HTTP status");
  const statusText = text.slice(marker + 1).trim();
  if (!/^\d{3}$/.test(statusText)) fail("verification subprocess returned an invalid HTTP status");
  return { body: text.slice(0, marker), status: Number(statusText) };
}

function curlTimeoutSeconds(timeoutMs) {
  return String(Math.max(1, Math.ceil(timeoutMs / 1000)));
}

export function retryableStatus(status) {
  return status === 404 || status === 429 || (status >= 500 && status <= 599);
}

export function safeRunner(runner, command, args, options) {
  return Promise.resolve().then(() => runner(command, args, {
    ...options,
    env: publicEnvironment(),
  })).catch((error) => {
    if (error instanceof VerificationError) throw error;
    throw new VerificationError("verification subprocess was unavailable", { retryable: true });
  });
}

export function defaultRunner(command, args, options) {
  return runReleaseProcess(command, args, options);
}

async function pauseForRetry(policy) {
  const delayMs = Math.max(0, Number(policy.delayMs) || 0);
  if (delayMs === 0) {
    remainingMs(policy);
    return;
  }
  const remaining = remainingMs(policy);
  if (delayMs >= remaining) fail("verification retry exceeds operation deadline");
  await new Promise((resolve) => setTimeout(resolve, delayMs));
}

export async function retryRequest(policy, operation, unavailableMessage) {
  let lastFailure;
  for (let attempt = 1; attempt <= policy.attempts; attempt += 1) {
    remainingMs(policy);
    try {
      const result = await operation(childTimeoutMs(policy));
      if (!result.retryable) return result;
      lastFailure = new VerificationError(unavailableMessage, { retryable: true });
    } catch (error) {
      if (!(error instanceof VerificationError) || !error.retryable) throw error;
      lastFailure = error;
    }
    if (attempt === policy.attempts) break;
    await pauseForRetry(policy);
  }
  throw lastFailure ?? new VerificationError(unavailableMessage, { retryable: true });
}

export function apiRequestArgs(version, timeoutMs) {
  const endpoint = version === undefined ? API_ROOT : API_ROOT + "/" + encodeURIComponent(version);
  return [
    "-q", "--silent", "--show-error", "--max-redirs", "0",
    "--connect-timeout", curlTimeoutSeconds(Math.min(timeoutMs, 10_000)),
    "--max-time", curlTimeoutSeconds(timeoutMs),
    "--user-agent", USER_AGENT,
    "--header", "Accept: application/json",
    "--write-out", "\n%{http_code}",
    endpoint,
  ];
}
export function crateRequestArgs(timeoutMs) { return apiRequestArgs(undefined, timeoutMs); }

export function staticRequestArgs(version, destination, timeoutMs) {
  return [
    "-q", "--silent", "--show-error", "--max-redirs", "0",
    "--connect-timeout", curlTimeoutSeconds(Math.min(timeoutMs, 10_000)),
    "--max-time", curlTimeoutSeconds(timeoutMs),
    "--max-filesize", String(MAX_ARCHIVE_BYTES),
    "--user-agent", USER_AGENT,
    "--output", destination,
    "--write-out", "\n%{http_code}",
    STATIC_ROOT + "/hardgate-" + encodeURIComponent(version) + ".crate",
  ];
}

export function validateMetadata(body, version, expectedSha256) {
  return validateMetadataImpl(body, version, expectedSha256, fail);
}
export function validateDefaultMetadata(body, version) {
  return validateDefaultMetadataImpl(body, version, fail);
}
function parseCargoInfo(output) {
  try {
    return JSON.parse(normalizeOutput(output));
  } catch {
    fail("local Cargo archive has malformed or duplicate .cargo_vcs_info.json data");
  }
}
function assertCleanCargoInfo(git) {
  if (git.dirty !== undefined && git.dirty !== false) fail("local Cargo archive has a dirty or malformed Cargo VCS identity");
}
function assertCargoInfo(info, sourceSha) {
  const git = info?.git;
  if (!info || Array.isArray(info) || typeof info !== "object" || !git || Array.isArray(git) || typeof git !== "object") {
    fail("local Cargo archive has an invalid Cargo VCS identity shape");
  }
  if (git.sha1 !== sourceSha || typeof git.sha1 !== "string" || !SOURCE_SHA_PATTERN.test(git.sha1)) {
    fail("local Cargo archive source identity does not match --source-sha");
  }
  assertCleanCargoInfo(git);
}
export async function readCargoVcsInfo({ archivePath, version, sourceSha, policy, run, tarCommand }) {
  const member = CRATE_NAME + "-" + version + "/.cargo_vcs_info.json";
  let listing;
  try {
    listing = await safeRunner(run, tarCommand, ["-tzf", archivePath], { timeoutMs: childTimeoutMs(policy), maxBuffer: MAX_TAR_OUTPUT_BYTES, cwd: path.dirname(archivePath) });
  } catch {
    fail("local Cargo archive has no readable .cargo_vcs_info.json identity");
  }
  const matches = normalizeOutput(listing).split(/\r?\n/).filter((entry) => entry === member).length;
  if (matches !== 1) fail("local Cargo archive has a missing or duplicate .cargo_vcs_info.json identity");
  let output;
  try {
    output = await safeRunner(run, tarCommand, ["-xOzf", archivePath, member], { timeoutMs: childTimeoutMs(policy), maxBuffer: MAX_TAR_OUTPUT_BYTES, cwd: path.dirname(archivePath) });
  } catch {
    fail("local Cargo archive has no readable .cargo_vcs_info.json identity");
  }
  assertCargoInfo(parseCargoInfo(output), sourceSha);
}
function splitArgument(argument) {
  const equals = argument.indexOf("=");
  return { flag: equals < 0 ? argument : argument.slice(0, equals), inline: equals < 0 ? undefined : argument.slice(equals + 1) };
}
function requiredValue(argv, index, inline) {
  const value = inline ?? argv[index + 1];
  if (typeof value !== "string" || value.length === 0 || (inline === undefined && value.startsWith("--"))) {
    fail("arguments must provide values for --expected, --version, and --source-sha");
  }
  return { value, nextIndex: inline === undefined ? index + 1 : index };
}
function parseOneArgument(argv, index, values, names) {
  const argument = argv[index];
  if (argument === "--require-default") {
    if (values.requireDefault) fail("arguments must contain each required option exactly once");
    values.requireDefault = true;
    return index;
  }
  const { flag, inline } = splitArgument(argument);
  const key = names.get(flag);
  if (!key || Object.hasOwn(values, key)) fail("arguments must contain each required option exactly once");
  const parsed = requiredValue(argv, index, inline);
  values[key] = parsed.value;
  return parsed.nextIndex;
}
export function parseArguments(argv) {
  const values = {};
  const names = new Map([["--expected", "expected"], ["--version", "version"], ["--source-sha", "sourceSha"]]);
  for (let index = 0; index < argv.length; index += 1) index = parseOneArgument(argv, index, values, names);
  if (!Object.hasOwn(values, "expected") || !Object.hasOwn(values, "version") || !Object.hasOwn(values, "sourceSha")) {
    fail("--expected, --version, and --source-sha are required");
  }
  values.requireDefault = Boolean(values.requireDefault);
  return values;
}
