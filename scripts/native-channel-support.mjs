// Shared validation, host, and proof primitives for native npm channel checks.
"use strict";

import crypto from "node:crypto";
import { spawnSync } from "node:child_process";
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

const HOST_PACKAGE_RULES = [
  ["linux", "x64", "glibc", "hardgate-linux-x64"],
  ["linux", "x64", "musl", "hardgate-linux-x64-musl"],
  ["linux", "arm64", "glibc", "hardgate-linux-arm64"],
  ["linux", "arm64", "musl", "hardgate-linux-arm64-musl"],
  ["darwin", "x64", null, "hardgate-darwin-x64"],
  ["darwin", "arm64", null, "hardgate-darwin-arm64"],
];

export const WRAPPER_PACKAGE = "@tech-byte-frontier/hardgate";
export const PUBLIC_NPM_REGISTRY = "https://registry.npmjs.org/";
export const PROOF_VERSION = 1;

const SHA = /^[0-9a-f]{40}$/;
const OPTION_NAMES = new Set(["package", "version", "source-sha", "archive", "mode", "output", "wrapper-source"]);

export function fail(message) {
  throw new Error(`verify-native-channel: ${message}`);
}

export function regularFile(file, label) {
  let stats;
  try {
    stats = fs.lstatSync(file);
  } catch (error) {
    fail(`${label} cannot be read: ${error.message}`);
  }
  if (stats.isSymbolicLink() || !stats.isFile()) fail(`${label} must be a regular file`);
  return stats;
}

export function packageDescriptor(packageName) {
  const descriptor = Object.hasOwn(NATIVE_PACKAGES, packageName) ? NATIVE_PACKAGES[packageName] : undefined;
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
    fail(`${label} must be a lowercase 40-character hexadecimal source identity`);
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

function parseOptionValues(argv) {
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
  return values;
}

function requireOptions(values) {
  for (const name of OPTION_NAMES) {
    if (name !== "wrapper-source" && !Object.hasOwn(values, name)) fail(`--${name} is required`);
  }
}

function normalizedArgs(values) {
  const packageName = packageDescriptor(values.package).name;
  const version = assertVersion(values.version);
  const sourceSha = assertSourceSha(values["source-sha"]);
  const mode = assertMode(values.mode);
  const archive = path.resolve(values.archive);
  const output = path.resolve(values.output);
  if (archive === output) fail("--archive and --output must identify different files");
  if (archive.includes("\0") || output.includes("\0")) fail("paths cannot contain NUL bytes");
  const wrapperSource = values["wrapper-source"] === undefined ? undefined : path.resolve(values["wrapper-source"]);
  if (wrapperSource?.includes("\0")) fail("--wrapper-source cannot contain NUL bytes");
  return { packageName, version, sourceSha, archive, mode, output, wrapperSource };
}

export function parseArgs(argv) {
  const values = parseOptionValues(argv);
  requireOptions(values);
  return normalizedArgs(values);
}

function linuxLibcEvidence({ glibcVersion, sharedObjects, lddOutput } = {}) {
  if (typeof glibcVersion === "string" && glibcVersion.trim()) return "glibc";
  if (Array.isArray(sharedObjects) && sharedObjects.some((value) => /(?:^|[\\/])(?:ld-musl-|libc\.musl-)/i.test(String(value)))) return "musl";
  if (typeof lddOutput === "string") {
    if (/\bmusl\b/i.test(lddOutput)) return "musl";
    if (/(?:\bglibc\b|GNU C Library|ldd \([^)]*GLIBC)/i.test(lddOutput)) return "glibc";
    return null;
  }
  const result = spawnSync("/usr/bin/ldd", ["--version"], {
    encoding: "utf8",
    timeout: 1_000,
    killSignal: "SIGKILL",
    env: { PATH: `${path.dirname(process.execPath)}:/usr/bin:/bin` },
    maxBuffer: 64 * 1024,
  });
  const output = `${result.stdout ?? ""}\n${result.stderr ?? ""}`;
  if (/\bmusl\b/i.test(output)) return "musl";
  if (/(?:\bglibc\b|GNU C Library|ldd \([^)]*GLIBC)/i.test(output)) return "glibc";
  return null;
}

export function detectHost({ platform = process.platform, arch = process.arch, glibcVersion, sharedObjects, lddOutput } = {}) {
  if (platform !== "linux") return { platform, arch, libc: null };
  let runtimeGlibc = glibcVersion;
  let runtimeSharedObjects = sharedObjects;
  if (runtimeGlibc === undefined) {
    try {
      const report = process.report?.getReport?.();
      runtimeGlibc = report?.header?.glibcVersionRuntime;
      runtimeSharedObjects ??= report?.sharedObjects;
    } catch {
      runtimeGlibc = null;
    }
  }
  return { platform, arch, libc: linuxLibcEvidence({ glibcVersion: runtimeGlibc, sharedObjects: runtimeSharedObjects, lddOutput }) };
}

export function assertHostSupports(descriptor, host) {
  if (!host || host.platform !== descriptor.platform || host.arch !== descriptor.arch) {
    fail(`${descriptor.name} cannot run on ${host?.platform ?? "unknown"}/${host?.arch ?? "unknown"}`);
  }
  if (descriptor.platform === "linux" && host.libc !== "glibc" && host.libc !== "musl") {
    fail(`Linux libc could not be identified for ${host.platform}/${host.arch}`);
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
  return HOST_PACKAGE_RULES.find(([platform, arch, libc]) => (
    host?.platform === platform && host?.arch === arch && (libc === null || host?.libc === libc)
  ))?.[3] ?? null;
}

export function needsNpmForce(descriptor, host) {
  assertHostSupports(descriptor, host);
  return descriptor.libc === "musl" && host.libc === "glibc";
}

export function npmPackageSpec(packageName, version, mode) {
  return `${packageName}@${mode === "exact" ? version : "latest"}`;
}

export function restrictedPath() {
  return `${path.dirname(process.execPath)}:/usr/bin:/bin`;
}

export function nodeNpmPath() {
  const candidate = path.join(path.dirname(process.execPath), "npm");
  if (!fs.existsSync(candidate)) fail(`npm is missing from the Node prefix: ${candidate}`);
  return candidate;
}

export function sanitizedEnvironment(source = process.env, { pathValue } = {}) {
  const result = { ...source };
  for (const key of Object.keys(result)) {
    const lower = key.toLowerCase();
    if (
      lower.startsWith("npm_config_") ||
      /(?:token|secret|password|credential|authorization|auth|private[_-]?key|github|proxy)/i.test(key)
    ) {
      delete result[key];
    }
  }
  delete result.HARDGATE_BINARY;
  delete result.HARDGATE_LAUNCHER_DEPTH;
  delete result.NODE_OPTIONS;
  delete result.NODE_PATH;
  delete result.NODE_TLS_REJECT_UNAUTHORIZED;
  delete result.TAR_OPTIONS;
  delete result.tar_options;
  result.PATH = pathValue ?? restrictedPath();
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

// Preserve the original support-module API while proof durability lives in
// its bounded helper module.
export { validateProof, writeProofAtomic } from "./native-channel-proof.mjs";
