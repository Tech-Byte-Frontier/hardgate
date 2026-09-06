// Bounded npm latest-channel promotion primitives. Dist-tag promotion is a
// registry mutation, not package publication or an OIDC provenance exchange.
"use strict";

import { PLATFORM_NAMES } from "./release-platforms.mjs";

import path from "node:path";
import fs from "node:fs";
import os from "node:os";
import { fileURLToPath } from "node:url";
import { compareReleaseTags } from "./release-order.mjs";
import { promoteVerifiedChannel } from "./channel-promotion.mjs";
import { childTimeoutMs, remainingMs } from "./npm-verification-policy.mjs";
import { runReleaseProcess } from "./release-process.mjs";

const NPM_PLATFORM_CHANNELS = PLATFORM_NAMES;
const NPM_WRAPPER_CHANNEL = "@tech-byte-frontier/hardgate";
export const NPM_CHANNELS = Object.freeze([...NPM_PLATFORM_CHANNELS, NPM_WRAPPER_CHANNEL]);
export const NPM_REGISTRY = "https://registry.npmjs.org";
const SAFE_ENVIRONMENT = new Set([
  "PATH", "HOME", "TMPDIR", "TMP", "TEMP", "LANG", "LC_ALL", "TZ", "CI",
  "NPM_VERIFY_ATTEMPTS", "NPM_VERIFY_DELAY_SECONDS", "NPM_VERIFY_TIMEOUT_SECONDS",
  "NPM_VERIFY_CHILD_TIMEOUT_SECONDS",
]);
const PLATFORM_SET = new Set(NPM_PLATFORM_CHANNELS);
const SEMVER_ERROR = "npm latest metadata has an invalid version";
const TOOLING_SCRIPTS = path.dirname(fileURLToPath(import.meta.url));

export class NpmPromotionError extends Error {
  constructor(code, message, cause) {
    super(message, cause === undefined ? undefined : { cause });
    this.name = "NpmPromotionError";
    this.code = code;
  }
}
const error = (code, message, cause) => new NpmPromotionError(code, message, cause);

function text(value, label) {
  if (typeof value !== "string" || value.length === 0 || value.trim() !== value) {
    throw error("npm_invalid_request", `${label} is required`);
  }
  return value;
}
function version(value) {
  text(value, "version");
  try { compareReleaseTags(`v${value}`, `v${value}`); }
  catch (cause) { throw error("npm_invalid_version", SEMVER_ERROR, cause); }
  return value;
}
function channel(name) {
  if (typeof name !== "string" || !NPM_CHANNELS.includes(name)) throw error("npm_invalid_channel", "npm channel is not supported");
  return name;
}
function policy(value) {
  if (value === null || typeof value !== "object") throw error("npm_invalid_policy", "npm promotion policy is invalid");
  try { remainingMs(value); } catch (cause) { throw error("npm_timeout", "npm promotion operation deadline is exhausted", cause); }
  return value;
}

export function credentialFreeEnvironment(source = process.env) {
  const result = {};
  for (const key of SAFE_ENVIRONMENT) if (typeof source[key] === "string") result[key] = source[key];
  if (!result.PATH) result.PATH = "/usr/local/bin:/usr/bin:/bin";
  return result;
}

function promotionResource(source = process.env) {
  const token = source.NODE_AUTH_TOKEN;
  if (typeof token !== "string" || token.trim().length === 0) throw error("npm_auth_missing", "npm promotion requires NODE_AUTH_TOKEN");
  return configResource(source, token);
}

function configResource(source, token) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-npm-config-"));
  const home = path.join(root, "home");
  const cache = path.join(root, "cache");
  const userConfig = path.join(root, "user.npmrc");
  const globalConfig = path.join(root, "global.npmrc");
  try {
    for (const directory of [home, cache]) { fs.mkdirSync(directory, { mode: 0o700 }); fs.chmodSync(directory, 0o700); }
    const contents = token === undefined ? `registry=${NPM_REGISTRY}\n` : `registry=${NPM_REGISTRY}\n//registry.npmjs.org/:_authToken=\${NODE_AUTH_TOKEN}\n`;
    fs.writeFileSync(userConfig, contents, { mode: 0o600 });
    fs.writeFileSync(globalConfig, "", { mode: 0o600 });
    fs.chmodSync(userConfig, 0o600); fs.chmodSync(globalConfig, 0o600);
    const environment = credentialFreeEnvironment(source);
    Object.assign(environment, { HOME: home, npm_config_cache: cache, npm_config_userconfig: userConfig, npm_config_globalconfig: globalConfig, npm_config_registry: NPM_REGISTRY });
    if (token !== undefined) {
      environment.NODE_AUTH_TOKEN = token;
      Object.assign(environment, { npm_config_audit: "false", npm_config_fund: "false", npm_config_fetch_retries: "0" });
    }
    return { environment, cleanup: () => fs.rmSync(root, { recursive: true, force: true }), root };
  } catch (cause) {
    fs.rmSync(root, { recursive: true, force: true });
    throw cause;
  }
}

export function latestUrl(name) {
  channel(name);
  return `${NPM_REGISTRY}/${encodeURIComponent(name)}/latest`;
}

function curlResponse(output) {
  if (typeof output !== "string") throw error("npm_latest_response", "npm latest response was not text");
  const match = output.match(/\n([0-9]{3})\s*$/u);
  if (!match) throw error("npm_latest_response", "npm latest response had no HTTP status");
  return { status: Number(match[1]), body: output.slice(0, match.index).trim() };
}
function metadataVersion(name, metadata) {
  if (metadata === null || typeof metadata !== "object" || Array.isArray(metadata)) throw error("npm_latest_metadata", "npm latest metadata was not an object");
  if (metadata.name !== name) throw error("npm_latest_identity", "npm latest metadata has the wrong package name");
  if (typeof metadata.version !== "string") throw error("npm_latest_metadata", SEMVER_ERROR);
  try { compareReleaseTags(`v${metadata.version}`, `v${metadata.version}`); }
  catch (cause) { throw error("npm_latest_metadata", SEMVER_ERROR, cause); }
  return metadata.version;
}

export async function probeNpmLatest({ name, policy: requestPolicy, sourceCwd, env = process.env, runProcess = runReleaseProcess }) {
  channel(name);
  policy(requestPolicy);
  const timeoutMs = childTimeoutMs(requestPolicy);
  const seconds = Math.max(1, Math.ceil(timeoutMs / 1000));
  const resource = configResource(env);
  let output;
  try {
    output = await runProcess("curl", [
      "--disable", "--location", "--silent", "--show-error", "--connect-timeout", String(seconds),
      "--max-time", String(seconds), "--write-out", "\n%{http_code}\n", latestUrl(name),
    ], { cwd: sourceCwd, env: resource.environment, timeoutMs });
  } catch (cause) {
    const wrapped = error("npm_latest_probe", "npm latest probe failed", cause);
    if (cause?.retryable || /(?:EAI_AGAIN|ECONNRESET|ETIMEDOUT|ECONNREFUSED|429|5\d\d)/iu.test(String(cause?.code ?? ""))) wrapped.retryable = true;
    throw wrapped;
  } finally {
    resource.cleanup();
  }
  const response = curlResponse(output);
  if (response.status === 404) return { state: "missing" };
  if (response.status !== 200) {
    const fatal = error("npm_latest_http", "npm latest probe returned a fatal HTTP status");
    if (response.status === 429 || response.status >= 500) fatal.retryable = true;
    throw fatal;
  }
  let metadata;
  try { metadata = JSON.parse(response.body); }
  catch (cause) { throw error("npm_latest_metadata", "npm latest metadata was not valid JSON", cause); }
  const observed = metadataVersion(name, metadata);
  return { state: "present", metadata: { name, version: observed }, version: observed };
}

function normalizeLatest(result, name, requested) {
  if (result === null || typeof result !== "object" || Array.isArray(result)) throw error("npm_latest_response", "npm latest probe returned an invalid result");
  if (result.state === "missing") {
    if (Object.keys(result).length !== 1) throw error("npm_latest_response", "npm latest missing result contains unexpected fields");
    return { state: "missing" };
  }
  if (result.state !== "present") throw error("npm_latest_response", "npm latest probe returned an unknown state");
  const observed = metadataVersion(name, result.metadata ?? result);
  const comparison = compareReleaseTags(`v${observed}`, `v${requested}`);
  if (comparison > 0) throw error("npm_latest_newer", "npm latest channel is newer than the requested release");
  if (comparison === 0 && observed !== requested) throw error("npm_latest_identity", "npm latest metadata is not the exact requested release");
  return { state: "present", version: observed };
}
function safeChild(code, message, cause) {
  const wrapped = error(code, message, cause);
  if (cause?.retryable) wrapped.retryable = true;
  return wrapped;
}
async function bounded(runProcess, command, args, options) {
  try { return await runProcess(command, args, { cwd: options.cwd, env: options.env, timeoutMs: options.timeoutMs ?? childTimeoutMs(options.policy) }); }
  catch (cause) { throw safeChild("npm_child_failed", "npm promotion subprocess failed", cause); }
}
function verifierArgs(name, releaseVersion, distDir) {
  const args = [path.join(TOOLING_SCRIPTS, "verify-npm-publication.mjs"), "--version", releaseVersion, "--dist", path.resolve(distDir)];
  if (PLATFORM_SET.has(name)) args.push("--platform-only", "--package", name);
  return args;
}

async function channelProbe(context) {
  if (context.fatalPromotionError) throw context.fatalPromotionError;
  try {
    const result = await context.probeLatest({
      name: context.name, version: context.version, sourceCwd: context.sourceCwd,
      policy: context.policy, env: credentialFreeEnvironment(context.env), runProcess: context.runProcess,
    });
    return normalizeLatest(result, context.name, context.version);
  } catch (cause) {
    if (cause instanceof NpmPromotionError) throw cause;
    throw safeChild("npm_latest_probe", "npm latest probe failed", cause);
  }
}
async function immutableProof(context) {
  const resource = configResource(context.env);
  try {
    const env = resource.environment;
    if (context.verifyImmutable) {
      const proof = await context.verifyImmutable({
        name: context.name, version: context.version, distDir: path.resolve(context.distDir),
        sourceCwd: context.sourceCwd, policy: context.policy, env, runProcess: context.runProcess,
      });
      if (proof === false || (proof && typeof proof === "object" && proof.verified === false)) throw error("npm_immutable_failed", "immutable npm payload verification failed");
    } else {
      await bounded(context.runProcess, process.execPath, verifierArgs(context.name, context.version, context.distDir), {
        policy: context.policy, cwd: context.sourceCwd, env, timeoutMs: remainingMs(context.policy),
      });
    }
    if (context.revalidate) context.revalidate();
    context.immutableVerified = true;
    return { verified: true };
  } catch (cause) {
    throw safeChild("npm_immutable_failed", "immutable npm payload verification failed", cause);
  } finally {
    resource.cleanup();
  }
}
async function defaultProof(context) {
  if (!context.immutableVerified) throw error("npm_default_mismatch", "latest verification lacks immutable payload proof");
  const observed = await channelProbe(context);
  if (observed.state !== "present" || observed.version !== context.version) throw error("npm_default_mismatch", "npm latest readback did not identify the requested release");
  return { verified: true, metadata: { name: context.name, version: context.version } };
}
async function mutateLatest(context) {
  let resource;
  try {
    resource = promotionResource(context.env);
    await bounded(context.runProcess, "npm", [
      "dist-tag", "add", `${context.name}@${context.version}`, "latest", `--registry=${NPM_REGISTRY}`,
    ], { policy: context.policy, cwd: context.sourceCwd, env: resource.environment });
    return { verified: true };
  } catch (cause) {
    context.lastMutationError = cause;
    if (cause instanceof NpmPromotionError && cause.code === "npm_auth_missing") {
      context.fatalPromotionError = cause;
      throw cause;
    }
    throw safeChild("npm_mutation_failed", "npm latest tag mutation failed", cause);
  } finally {
    resource?.cleanup();
  }
}

function npmChannelOperations(options) {
  const context = {
    name: channel(options.name), version: version(options.version),
    distDir: text(options.distDir, "dist directory"), sourceCwd: text(options.sourceCwd, "source directory"),
    policy: policy(options.policy), env: options.env ?? process.env,
    runProcess: options.runProcess ?? runReleaseProcess, probeLatest: options.probeLatest ?? probeNpmLatest,
    verifyImmutable: options.verifyImmutable, revalidate: options.revalidate, immutableVerified: false,
    lastMutationError: undefined, fatalPromotionError: undefined,
  };
  return {
    probe: () => channelProbe(context),
    promote: () => mutateLatest(context),
    verifyDefault: () => defaultProof(context),
    verifyImmutable: () => immutableProof(context),
    getLastMutationError: () => context.lastMutationError,
  };
}

export async function promoteNpmChannel(options) {
  const operations = npmChannelOperations(options);
  try {
    const result = await promoteVerifiedChannel({ version: options.version, exactConsumerVerified: options.exactConsumerVerified, policy: options.policy }, operations);
    return { ...result, operations };
  } catch (cause) {
    const mutation = operations.getLastMutationError();
    if (cause instanceof NpmPromotionError) {
      if (mutation && !cause.cause) cause.cause = mutation;
      throw cause;
    }
    const message = String(cause?.message ?? "");
    if (/requested version was not observed|target probe failed/iu.test(message)) throw error("npm_readback_failed", "npm latest tag readback did not identify the requested release", mutation ?? cause);
    if (/deadline/iu.test(message)) throw error("npm_timeout", "npm promotion operation deadline exhausted", mutation ?? cause);
    if (/default consumer verification/iu.test(message)) throw error("npm_default_mismatch", "npm latest selector verification failed", mutation ?? cause);
    if (/immutable verification/iu.test(message)) throw error("npm_immutable_failed", "immutable npm payload verification failed", mutation ?? cause);
    throw error("npm_channel_failed", "npm channel promotion failed", mutation ?? cause);
  }
}
