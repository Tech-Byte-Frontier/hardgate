// Select one explicit npm publication credential mode and bound its environment.
"use strict";

const AUTH_ENV_KEYS = [
  "NODE_AUTH_TOKEN",
  "NPM_TOKEN",
  "GITHUB_TOKEN",
  "GH_TOKEN",
  "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
  "ACTIONS_ID_TOKEN_REQUEST_URL",
];
const PROBE_AUTH_KEYS = new Set(AUTH_ENV_KEYS);
const TRUSTED_PUBLISHER_KEYS = new Set([
  "NODE_AUTH_TOKEN",
  "NPM_TOKEN",
  "GITHUB_TOKEN",
  "GH_TOKEN",
]);
const TOKEN_NODE_MINIMUM = "18.0.0";
const TRUSTED_NODE_MINIMUM = "22.14.0";
const TRUSTED_NPM_MINIMUM = "11.5.1";
const SEMVER_PATTERN = /^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;

function normalizedMode(mode) {
  if (mode === undefined) return "token";
  if (mode === "token" || mode === "trusted") return mode;
  throw new Error("npm publisher auth mode must be token or trusted");
}

function requireEnvironment(env) {
  if (env === null || typeof env !== "object") throw new TypeError("npm publisher auth env must be an object");
  return env;
}

function hasText(value) {
  return typeof value === "string" && value.trim().length > 0;
}

function withoutKeys(env, keys) {
  const result = { ...env };
  for (const key of keys) delete result[key];
  return result;
}

function redactEnvironment(env) {
  const result = { ...env };
  for (const key of AUTH_ENV_KEYS) {
    if (Object.hasOwn(result, key)) result[key] = "[REDACTED_SECRET]";
  }
  return result;
}

function makeJsonSafeEnvironment(env) {
  Object.defineProperty(env, "toJSON", {
    configurable: false,
    enumerable: false,
    value() {
      return redactEnvironment(this);
    },
    writable: false,
  });
  return env;
}

function requireHttpsUrl(value) {
  if (!hasText(value) || value !== value.trim()) throw new Error("trusted npm publisher requires a valid HTTPS OIDC request URL");
  let parsed;
  try {
    parsed = new URL(value);
  } catch {
    throw new Error("trusted npm publisher requires a valid HTTPS OIDC request URL");
  }
  if (parsed.protocol !== "https:" || !parsed.hostname) throw new Error("trusted npm publisher requires a valid HTTPS OIDC request URL");
  return value;
}

function parseVersion(value, label) {
  if (typeof value !== "string") throw new Error(`${label} version must be a strict semantic version`);
  const match = SEMVER_PATTERN.exec(value);
  if (!match || match.slice(1, 4).some((part) => part.length > 1 && part.startsWith("0"))) {
    throw new Error(`${label} version must be a strict semantic version`);
  }
  const prerelease = match[4]?.split(".") ?? [];
  if (prerelease.some((part) => part.length === 0 || (/^\d+$/.test(part) && part.length > 1 && part.startsWith("0")))) {
    throw new Error(`${label} version must be a strict semantic version`);
  }
  return {
    major: Number(match[1]),
    minor: Number(match[2]),
    patch: Number(match[3]),
    prerelease,
    value,
  };
}

function compareCoreVersions(left, right) {
  for (const key of ["major", "minor", "patch"]) {
    if (left[key] !== right[key]) return left[key] > right[key] ? 1 : -1;
  }
  return 0;
}

function comparePrereleaseIdentifiers(left, right) {
  if (left === right) return 0;
  const leftNumeric = /^\d+$/.test(left);
  const rightNumeric = /^\d+$/.test(right);
  if (leftNumeric && rightNumeric) return Number(left) > Number(right) ? 1 : -1;
  if (leftNumeric !== rightNumeric) return leftNumeric ? -1 : 1;
  return left > right ? 1 : -1;
}

function comparePrereleaseVersions(left, right) {
  if (left.length === 0 && right.length === 0) return 0;
  if (left.length === 0) return 1;
  if (right.length === 0) return -1;
  for (let index = 0; index < Math.max(left.length, right.length); index += 1) {
    if (index >= left.length) return -1;
    if (index >= right.length) return 1;
    const comparison = comparePrereleaseIdentifiers(left[index], right[index]);
    if (comparison !== 0) return comparison;
  }
  return 0;
}

function compareVersions(left, right) {
  const coreComparison = compareCoreVersions(left, right);
  return coreComparison === 0 ? comparePrereleaseVersions(left.prerelease, right.prerelease) : coreComparison;
}

function requireMinimum(actual, minimum, label) {
  if (compareVersions(actual, parseVersion(minimum, label)) < 0) throw new Error(`${label} version is below the supported publisher floor`);
}

export function npmPublisherAuth(mode, env = process.env) {
  const selectedMode = normalizedMode(mode);
  const source = requireEnvironment(env);
  const probeEnv = makeJsonSafeEnvironment(withoutKeys(source, PROBE_AUTH_KEYS));
  const publishEnv = { ...source };

  for (const key of TRUSTED_PUBLISHER_KEYS) delete publishEnv[key];
  delete publishEnv["ACTIONS_ID_TOKEN_REQUEST_TOKEN"];
  delete publishEnv["ACTIONS_ID_TOKEN_REQUEST_URL"];

  if (selectedMode === "token") {
    if (!hasText(source.NODE_AUTH_TOKEN)) throw new Error("token npm publisher requires NODE_AUTH_TOKEN");
    publishEnv.NODE_AUTH_TOKEN = source.NODE_AUTH_TOKEN;
  } else {
    if (source.GITHUB_ACTIONS !== "true") throw new Error("trusted npm publisher requires GitHub Actions");
    requireHttpsUrl(source.ACTIONS_ID_TOKEN_REQUEST_URL);
    if (!hasText(source.ACTIONS_ID_TOKEN_REQUEST_TOKEN)) throw new Error("trusted npm publisher requires the GitHub OIDC request token");
    publishEnv.ACTIONS_ID_TOKEN_REQUEST_URL = source.ACTIONS_ID_TOKEN_REQUEST_URL;
    publishEnv.ACTIONS_ID_TOKEN_REQUEST_TOKEN = source.ACTIONS_ID_TOKEN_REQUEST_TOKEN;
  }

  return { publishEnv: makeJsonSafeEnvironment(publishEnv), probeEnv, mode: selectedMode };
}

export function validateNpmPublisherToolchain(mode, versions = {}) {
  const selectedMode = normalizedMode(mode);
  if (versions === null || typeof versions !== "object") throw new TypeError("npm publisher toolchain versions must be an object");
  const node = parseVersion(versions.nodeVersion, "Node");
  const npm = parseVersion(versions.npmVersion, "npm");
  if (selectedMode === "trusted") {
    requireMinimum(node, TRUSTED_NODE_MINIMUM, "Node");
    requireMinimum(npm, TRUSTED_NPM_MINIMUM, "npm");
  } else {
    requireMinimum(node, TOKEN_NODE_MINIMUM, "Node");
  }
  return { mode: selectedMode, nodeVersion: node.value, npmVersion: npm.value };
}
