// Shared validation, host, and proof primitives for native npm channel checks.
"use strict";

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { compareReleaseTags } from "./release-order.mjs";

export const NATIVE_PACKAGES = Object.freeze({
  "hardgate-linux-x64": Object.freeze({
    name: "hardgate-linux-x64",
    platform: "linux",
    arch: "x64",
    libc: "glibc",
    target: "x86_64-unknown-linux-gnu",
    archPattern: /x86-64/,
    abi: "gnu",
  }),
  "hardgate-linux-x64-musl": Object.freeze({
    name: "hardgate-linux-x64-musl",
    platform: "linux",
    arch: "x64",
    libc: "musl",
    target: "x86_64-unknown-linux-musl",
    archPattern: /x86-64/,
    abi: "musl",
  }),
  "hardgate-linux-arm64": Object.freeze({
    name: "hardgate-linux-arm64",
    platform: "linux",
    arch: "arm64",
    libc: "glibc",
    target: "aarch64-unknown-linux-gnu",
    archPattern: /ARM aarch64/,
    abi: "gnu",
  }),
  "hardgate-linux-arm64-musl": Object.freeze({
    name: "hardgate-linux-arm64-musl",
    platform: "linux",
    arch: "arm64",
    libc: "musl",
    target: "aarch64-unknown-linux-musl",
    archPattern: /ARM aarch64/,
    abi: "musl",
  }),
  "hardgate-darwin-x64": Object.freeze({
    name: "hardgate-darwin-x64",
    platform: "darwin",
    arch: "x64",
    libc: null,
    target: "x86_64-apple-darwin",
    archPattern: /x86_64/,
    abi: null,
  }),
  "hardgate-darwin-arm64": Object.freeze({
    name: "hardgate-darwin-arm64",
    platform: "darwin",
    arch: "arm64",
    libc: null,
    target: "aarch64-apple-darwin",
    archPattern: /arm64/,
    abi: null,
  }),
});

export const WRAPPER_PACKAGE = "@tech-byte-frontier/hardgate";
export const PUBLIC_NPM_REGISTRY = "https://registry.npmjs.org/";
export const PROOF_VERSION = 1;

const SHA = /^[0-9a-f]{40}(?:[0-9a-f]{24})?$/;
const HASH = /^[0-9a-f]{64}$/;
const OPTION_NAMES = new Set(["package", "version", "source-sha", "archive", "mode", "output"]);

export function fail(message) {
  throw new Error(`verify-native-channel: ${message}`);
}

export function packageDescriptor(packageName) {
  const descriptor = NATIVE_PACKAGES[packageName];
  if (!descriptor) fail(`--package must identify one of the six native packages, got ${packageName || "<missing>"}`);
  return descriptor;
}

export function assertVersion(version, label = "version") {
  if (typeof version !== "string" || version.length === 0) fail(`${label} is required`);
  try {
    compareReleaseTags(`v${version}`, `v${version}`);
  } catch {
    fail(`${label} must be a valid repository semantic version`);
  }
  return version;
}

export function assertSourceSha(sourceSha, label = "source-sha") {
  if (typeof sourceSha !== "string" || !SHA.test(sourceSha)) {
    fail(`${label} must be a lowercase 40- or 64-character hexadecimal source identity`);
  }
  return sourceSha;
}

function assertMode(mode) {
  if (mode !== "exact" && mode !== "default") fail(`--mode must be exact or default, got ${mode || "<missing>"}`);
  return mode;
}

function optionValue(argv, index, name, inlineValue) {
  const value = inlineValue ?? argv[index + 1];
  if (value === undefined || value.length === 0 || (inlineValue === undefined && value.startsWith("--"))) {
    fail(`${name} requires a value`);
  }
  return value;
}

export function parseArgs(argv) {
  const values = {};
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (!argument.startsWith("--")) fail(`unexpected argument ${argument}`);
    const equals = argument.indexOf("=");
    const rawName = equals < 0 ? argument.slice(2) : argument.slice(2, equals);
    if (!OPTION_NAMES.has(rawName)) fail(`unknown option --${rawName}`);
    if (Object.hasOwn(values, rawName)) fail(`duplicate option --${rawName}`);
    const inlineValue = equals < 0 ? undefined : argument.slice(equals + 1);
    values[rawName] = optionValue(argv, index, `--${rawName}`, inlineValue);
    if (inlineValue === undefined) index += 1;
  }
  for (const name of OPTION_NAMES) if (!Object.hasOwn(values, name)) fail(`--${name} is required`);
  const packageName = packageDescriptor(values.package).name;
  const version = assertVersion(values.version);
  const sourceSha = assertSourceSha(values["source-sha"]);
  const mode = assertMode(values.mode);
  const archive = path.resolve(values.archive);
  const output = path.resolve(values.output);
  if (archive === output) fail("--archive and --output must identify different files");
  if (archive.includes("\0") || output.includes("\0")) fail("paths cannot contain NUL bytes");
  return { packageName, version, sourceSha, archive, mode, output };
}

export function detectHost({ platform = process.platform, arch = process.arch, glibcVersion } = {}) {
  if (platform !== "linux") return { platform, arch, libc: null };
  let runtimeGlibc = glibcVersion;
  if (runtimeGlibc === undefined) {
    try {
      runtimeGlibc = process.report?.getReport?.().header?.glibcVersionRuntime;
    } catch {
      runtimeGlibc = null;
    }
  }
  return { platform, arch, libc: typeof runtimeGlibc === "string" && runtimeGlibc.trim() ? "glibc" : "musl" };
}

export function assertHostSupports(descriptor, host) {
  if (!host || host.platform !== descriptor.platform || host.arch !== descriptor.arch) {
    fail(`${descriptor.name} cannot run on ${host?.platform ?? "unknown"}/${host?.arch ?? "unknown"}`);
  }
  if (descriptor.platform === "linux" && descriptor.libc === "glibc" && host.libc !== "glibc") {
    fail(`${descriptor.name} requires glibc on a ${host.libc ?? "unknown"} host`);
  }
  return true;
}

export function wrapperHost(host) {
  return host?.platform === "linux" && host?.arch === "x64" && host?.libc === "glibc";
}

export function hostNativePackage(host) {
  if (host?.platform === "linux" && host?.arch === "x64" && host?.libc === "glibc") return "hardgate-linux-x64";
  if (host?.platform === "linux" && host?.arch === "x64" && host?.libc === "musl") return "hardgate-linux-x64-musl";
  if (host?.platform === "linux" && host?.arch === "arm64" && host?.libc === "glibc") return "hardgate-linux-arm64";
  if (host?.platform === "linux" && host?.arch === "arm64" && host?.libc === "musl") return "hardgate-linux-arm64-musl";
  if (host?.platform === "darwin" && host?.arch === "x64") return "hardgate-darwin-x64";
  if (host?.platform === "darwin" && host?.arch === "arm64") return "hardgate-darwin-arm64";
  return null;
}

export function needsNpmForce(descriptor, host) {
  assertHostSupports(descriptor, host);
  return descriptor.libc === "musl" && host.libc === "glibc";
}

export function npmPackageSpec(packageName, version, mode) {
  return `${packageName}@${mode === "exact" ? version : "latest"}`;
}

export function sanitizedEnvironment(source = process.env) {
  const result = { ...source };
  for (const key of Object.keys(result)) {
    const lower = key.toLowerCase();
    if (
      lower.startsWith("npm_config_") ||
      /(?:token|secret|password|credential|authorization|auth|private[_-]?key|github)/i.test(key)
    ) {
      delete result[key];
    }
  }
  delete result.HARDGATE_BINARY;
  delete result.HARDGATE_LAUNCHER_DEPTH;
  delete result.NPM_CONFIG_USERCONFIG;
  delete result.npm_config_userconfig;
  return result;
}

export function digestBytes(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex");
}

export function digestFile(file) {
  return digestBytes(fs.readFileSync(file));
}

export function pathInside(root, candidate) {
  const rootPath = fs.realpathSync(root);
  const candidatePath = fs.realpathSync(candidate);
  const relative = path.relative(rootPath, candidatePath);
  return relative.length > 0 && relative !== "." && !relative.startsWith(`..${path.sep}`) && relative !== ".." && !path.isAbsolute(relative);
}

export function stableExecutablePath(packageName, suffix = "bin/hardgate") {
  return `node_modules/${packageName}/${suffix}`.replaceAll(path.sep, "/");
}

export function assertStableExecutablePath(executable, label = "executable") {
  if (
    typeof executable !== "string" ||
    executable.length === 0 ||
    executable.length > 1024 ||
    executable.includes("\\") ||
    executable.includes("\0") ||
    executable.startsWith("/") ||
    path.posix.normalize(executable) !== executable ||
    !executable.startsWith("node_modules/")
  ) {
    fail(`${label} must be a normalized package-relative path`);
  }
  return executable;
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

export function validateProof(proof) {
  if (proof === null || typeof proof !== "object" || Array.isArray(proof)) fail("proof must be an object");
  const keys = Object.keys(proof).sort();
  const expected = ["consumer", "mode", "package", "source_sha", "version"];
  const withWrapper = [...expected, "wrapper"].sort();
  if (keys.join("\n") !== expected.sort().join("\n") && keys.join("\n") !== withWrapper.join("\n")) {
    fail("proof has unexpected fields");
  }
  const packageName = packageDescriptor(proof.package).name;
  const version = assertVersion(proof.version);
  const sourceSha = assertSourceSha(proof.source_sha, "proof.source_sha");
  const mode = assertMode(proof.mode);
  const result = {
    version,
    source_sha: sourceSha,
    mode,
    package: packageName,
    consumer: assertProofConsumer(proof.consumer, "proof.consumer"),
  };
  if (Object.hasOwn(proof, "wrapper")) result.wrapper = assertProofConsumer(proof.wrapper, "proof.wrapper");
  return result;
}

function writableTarget(target) {
  try {
    const stats = fs.lstatSync(target);
    if (stats.isSymbolicLink() || !stats.isFile()) fail("--output must identify a regular file");
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
    if (![
      "EINVAL",
      "ENOTSUP",
      "EISDIR",
    ].includes(error.code)) throw error;
  }
}

export function writeProofAtomic(output, proof) {
  const target = path.resolve(output);
  const checked = validateProof(proof);
  const directory = path.dirname(target);
  fs.mkdirSync(directory, { recursive: true, mode: 0o700 });
  writableTarget(target);
  const temporary = path.join(directory, `.${path.basename(target)}.${process.pid}.${crypto.randomBytes(12).toString("hex")}.tmp`);
  const bytes = Buffer.from(`${JSON.stringify(checked, null, 2)}\n`, "utf8");
  let descriptor;
  try {
    descriptor = fs.openSync(temporary, "wx", 0o600);
    fs.writeFileSync(descriptor, bytes);
    fs.fsyncSync(descriptor);
    fs.closeSync(descriptor);
    descriptor = undefined;
    writableTarget(target);
    fs.renameSync(temporary, target);
    fs.chmodSync(target, 0o600);
    syncDirectory(directory);
  } catch (error) {
    if (descriptor !== undefined) fs.closeSync(descriptor);
    try {
      fs.unlinkSync(temporary);
    } catch (cleanupError) {
      if (cleanupError.code !== "ENOENT") throw error;
    }
    throw error;
  }
  return checked;
}
