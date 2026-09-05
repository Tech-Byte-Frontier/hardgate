// Select one explicit npm publication credential mode and bound its environment.
// Registry authentication and npm provenance attestation use independent credentials:
// token mode may carry a valid GitHub OIDC pair for provenance, while probes never do.
"use strict";

import { compareReleaseTags } from "./release-order.mjs";

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
  if (parsed.protocol !== "https:" || !parsed.hostname || parsed.username || parsed.password || parsed.hash || value.includes("#")) {
    throw new Error("trusted npm publisher requires a valid HTTPS OIDC request URL");
  }
  return value;
}

function normalizeVersion(value, label) {
  if (typeof value !== "string" || !hasText(value) || value !== value.trim()) {
    throw new Error(`${label} version must be a strict semantic version`);
  }
  const tag = value.startsWith("v") ? value : `v${value}`;
  try {
    compareReleaseTags(tag, tag);
  } catch {
    throw new Error(`${label} version must be a strict semantic version`);
  }
  return { tag, value };
}

function requireMinimum(actual, minimum, label) {
  if (compareReleaseTags(actual.tag, normalizeVersion(minimum, label).tag) < 0) {
    throw new Error(`${label} version is below the supported publisher floor`);
  }
}

function optionalAttestationOidc(env) {
  const hasUrl = Object.hasOwn(env, "ACTIONS_ID_TOKEN_REQUEST_URL");
  const hasToken = Object.hasOwn(env, "ACTIONS_ID_TOKEN_REQUEST_TOKEN");
  if (!hasUrl && !hasToken) return undefined;
  if (env.GITHUB_ACTIONS !== "true") throw new Error("token npm publisher OIDC attestation requires GitHub Actions");
  const url = requireHttpsUrl(env.ACTIONS_ID_TOKEN_REQUEST_URL);
  if (!hasText(env.ACTIONS_ID_TOKEN_REQUEST_TOKEN)) throw new Error("token npm publisher OIDC attestation requires a complete OIDC credential pair");
  return { url, token: env.ACTIONS_ID_TOKEN_REQUEST_TOKEN };
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
    const attestation = optionalAttestationOidc(source);
    if (attestation) {
      publishEnv.ACTIONS_ID_TOKEN_REQUEST_URL = attestation.url;
      publishEnv.ACTIONS_ID_TOKEN_REQUEST_TOKEN = attestation.token;
    }
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
  const node = normalizeVersion(versions.nodeVersion, "Node");
  const npm = normalizeVersion(versions.npmVersion, "npm");
  if (selectedMode === "trusted") {
    requireMinimum(node, TRUSTED_NODE_MINIMUM, "Node");
    requireMinimum(npm, TRUSTED_NPM_MINIMUM, "npm");
  } else {
    requireMinimum(node, TOKEN_NODE_MINIMUM, "Node");
  }
  return { mode: selectedMode, nodeVersion: node.value, npmVersion: npm.value };
}
