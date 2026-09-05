"use strict";

import assert from "node:assert/strict";
import { npmPublisherAuth, validateNpmPublisherToolchain } from "../scripts/npm-publisher-auth.mjs";

const secrets = {
  npm: "npm-secret-value",
  legacy: "legacy-npm-secret",
  github: "github-secret-value",
  oidc: "oidc-request-secret",
};
const oidcUrl = "https://token.actions.githubusercontent.com/oidc";

function baseEnvironment() {
  return {
    PATH: "/usr/bin",
    NPM_CONFIG_REGISTRY: "https://registry.npmjs.org",
    GITHUB_ACTIONS: "true",
    NODE_AUTH_TOKEN: secrets.npm,
    NPM_TOKEN: secrets.legacy,
    GITHUB_TOKEN: secrets.github,
    GH_TOKEN: "gh-token-secret",
    ACTIONS_ID_TOKEN_REQUEST_URL: oidcUrl,
    ACTIONS_ID_TOKEN_REQUEST_TOKEN: secrets.oidc,
  };
}

function assertNoSecrets(value) {
  const serialized = typeof value === "string" ? value : value instanceof Error ? `${value.name}: ${value.message}` : JSON.stringify(value);
  for (const secret of Object.values(secrets)) assert.doesNotMatch(serialized, new RegExp(secret));
  assert.doesNotMatch(serialized, /gh-token-secret/);
}

function thrown(action) {
  let error;
  try {
    action();
  } catch (candidate) {
    error = candidate;
  }
  assert.ok(error instanceof Error, "expected an error");
  return error;
}

function assertProbeIsCredentialFree(environment) {
  for (const key of [
    "NODE_AUTH_TOKEN",
    "NPM_TOKEN",
    "GITHUB_TOKEN",
    "GH_TOKEN",
    "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
    "ACTIONS_ID_TOKEN_REQUEST_URL",
  ]) assert.equal(Object.hasOwn(environment, key), false, `probe environment must omit ${key}`);
}

const tokenInput = baseEnvironment();
const tokenSnapshot = { ...tokenInput };
const tokenAuth = npmPublisherAuth(undefined, tokenInput);
assert.equal(tokenAuth.mode, "token", "omitted mode keeps token compatibility");
assert.equal(tokenAuth.publishEnv.NODE_AUTH_TOKEN, secrets.npm);
assert.equal(Object.hasOwn(tokenAuth.publishEnv, "NPM_TOKEN"), false);
assert.equal(Object.hasOwn(tokenAuth.publishEnv, "GITHUB_TOKEN"), false);
assert.equal(Object.hasOwn(tokenAuth.publishEnv, "GH_TOKEN"), false);
assert.equal(Object.hasOwn(tokenAuth.publishEnv, "ACTIONS_ID_TOKEN_REQUEST_TOKEN"), false);
assert.equal(Object.hasOwn(tokenAuth.publishEnv, "ACTIONS_ID_TOKEN_REQUEST_URL"), false);
assert.equal(tokenAuth.publishEnv.PATH, "/usr/bin");
assertProbeIsCredentialFree(tokenAuth.probeEnv);
assert.deepEqual(tokenInput, tokenSnapshot, "token selection must not mutate the caller environment");

const trustedInput = baseEnvironment();
const trustedSnapshot = { ...trustedInput };
const trustedAuth = npmPublisherAuth("trusted", trustedInput);
assert.equal(trustedAuth.mode, "trusted");
assert.equal(trustedAuth.publishEnv.ACTIONS_ID_TOKEN_REQUEST_URL, oidcUrl);
assert.equal(trustedAuth.publishEnv.ACTIONS_ID_TOKEN_REQUEST_TOKEN, secrets.oidc);
assert.equal(trustedAuth.publishEnv.GITHUB_ACTIONS, "true");
for (const key of ["NODE_AUTH_TOKEN", "NPM_TOKEN", "GITHUB_TOKEN", "GH_TOKEN"]) {
  assert.equal(Object.hasOwn(trustedAuth.publishEnv, key), false, `trusted environment must omit ${key}`);
}
assertProbeIsCredentialFree(trustedAuth.probeEnv);
assert.deepEqual(trustedInput, trustedSnapshot, "trusted selection must not mutate the caller environment");

assert.throws(() => npmPublisherAuth("unknown", tokenInput), /mode must be token or trusted/);
assert.throws(() => npmPublisherAuth("", tokenInput), /mode must be token or trusted/);
assert.throws(() => npmPublisherAuth(undefined, { PATH: "/usr/bin" }), /requires NODE_AUTH_TOKEN/);
assert.throws(() => npmPublisherAuth("token", { NODE_AUTH_TOKEN: "   " }), /requires NODE_AUTH_TOKEN/);
assert.throws(
  () => npmPublisherAuth("token", { NPM_TOKEN: secrets.legacy, ACTIONS_ID_TOKEN_REQUEST_TOKEN: secrets.oidc, GITHUB_ACTIONS: "true" }),
  /requires NODE_AUTH_TOKEN/,
);

for (const environment of [
  { ACTIONS_ID_TOKEN_REQUEST_URL: "http://token.actions.githubusercontent.com", ACTIONS_ID_TOKEN_REQUEST_TOKEN: secrets.oidc, GITHUB_ACTIONS: "true" },
  { ACTIONS_ID_TOKEN_REQUEST_URL: "not-a-url", ACTIONS_ID_TOKEN_REQUEST_TOKEN: secrets.oidc, GITHUB_ACTIONS: "true" },
  { ACTIONS_ID_TOKEN_REQUEST_URL: "https://", ACTIONS_ID_TOKEN_REQUEST_TOKEN: secrets.oidc, GITHUB_ACTIONS: "true" },
  { ACTIONS_ID_TOKEN_REQUEST_URL: oidcUrl, ACTIONS_ID_TOKEN_REQUEST_TOKEN: secrets.oidc, GITHUB_ACTIONS: "false" },
  { ACTIONS_ID_TOKEN_REQUEST_URL: oidcUrl, GITHUB_ACTIONS: "true" },
]) {
  const error = thrown(() => npmPublisherAuth("trusted", environment));
  assertNoSecrets(error);
}
assert.throws(
  () => npmPublisherAuth("trusted", { NODE_AUTH_TOKEN: secrets.npm, NPM_TOKEN: secrets.legacy }),
  /requires GitHub Actions/,
);
assert.throws(
  () => npmPublisherAuth("trusted", { GITHUB_ACTIONS: "true", NODE_AUTH_TOKEN: secrets.npm, NPM_TOKEN: secrets.legacy }),
  /requires a valid HTTPS OIDC request URL/,
);

assertNoSecrets(JSON.stringify(tokenAuth));
assertNoSecrets(JSON.stringify(trustedAuth));
for (const error of [
  thrown(() => npmPublisherAuth("trusted", { GITHUB_ACTIONS: "true", ACTIONS_ID_TOKEN_REQUEST_URL: oidcUrl })),
  thrown(() => npmPublisherAuth("token", {})),
]) assertNoSecrets(error);

assert.deepEqual(validateNpmPublisherToolchain(undefined, { nodeVersion: "18.0.0", npmVersion: "12.0.2" }), {
  mode: "token",
  nodeVersion: "18.0.0",
  npmVersion: "12.0.2",
});
assert.doesNotThrow(() => validateNpmPublisherToolchain("token", { nodeVersion: "v18.0.0", npmVersion: "10.0.0" }));
assert.throws(() => validateNpmPublisherToolchain("token", { nodeVersion: "17.9.9", npmVersion: "12.0.2" }), /Node version is below/);
assert.doesNotThrow(() => validateNpmPublisherToolchain("trusted", { nodeVersion: "22.14.0", npmVersion: "11.5.1" }));
assert.doesNotThrow(() => validateNpmPublisherToolchain("trusted", { nodeVersion: "22.14.1", npmVersion: "11.5.2" }));
assert.throws(() => validateNpmPublisherToolchain("trusted", { nodeVersion: "22.13.9", npmVersion: "11.5.1" }), /Node version is below/);
assert.throws(() => validateNpmPublisherToolchain("trusted", { nodeVersion: "22.14.0", npmVersion: "11.5.0" }), /npm version is below/);

for (const versions of [
  { nodeVersion: "", npmVersion: "12.0.2" },
  { nodeVersion: "18.0", npmVersion: "12.0.2" },
  { nodeVersion: "18.0.0.1", npmVersion: "12.0.2" },
  { nodeVersion: "18.0.0-beta..1", npmVersion: "12.0.2" },
  { nodeVersion: "18.0.0", npmVersion: "12" },
  { nodeVersion: "18.0.0", npmVersion: "01.2.3" },
]) {
  assert.throws(() => validateNpmPublisherToolchain("token", versions), /version must be a strict semantic version/);
}
assert.throws(() => validateNpmPublisherToolchain("bad", { nodeVersion: "18.0.0", npmVersion: "12.0.2" }), /mode must be token or trusted/);
assertNoSecrets(JSON.stringify({ error: "auth validation failed" }));

console.log("npm_publisher_auth.test: OK (explicit modes, credential scope, no fallback, toolchain floors, redaction)");
