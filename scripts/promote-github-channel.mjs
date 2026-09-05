#!/usr/bin/env node
// Promote an independently verified GitHub release to the public default channel.
"use strict";

import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";
import { expectedGithubAssets } from "./github-staging-state.mjs";
import { promoteGithubChannel, validateGithubPromotionReceipt, GITHUB_CHANNEL } from "./github-channel-promotion.mjs";
import { compareReleaseTags } from "./release-order.mjs";
import { runReleaseProcess } from "./release-process.mjs";
import { projectRoot } from "./release-support.mjs";
import {
  readReceipt,
  recordFailure,
  writeReceiptAtomicSync,
} from "./release-receipt.mjs";

const OVERALL_TIMEOUT_MS = 20 * 60 * 1000;
const SUBPROCESS_TIMEOUT_MS = 60 * 1000;
const MAX_ASSET_BYTES = 1024 * 1024 * 1024;
const HASH_CHUNK_BYTES = 64 * 1024;
const READ_FLAGS = fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW ?? 0);
const EXPECTED_FAILURE_CODE = "github-promotion-failed";
const EXPECTED_FAILURE_MESSAGE = "GitHub channel promotion failed";

function fail(message) {
  throw new Error(`promote-github-channel: ${message}`);
}

function remainingMs(policy) {
  const remaining = Math.floor(policy.deadline - performance.now());
  if (remaining < 1) fail("operation deadline exhausted");
  return remaining;
}

function readOption(argv, index, options) {
  const argument = argv[index];
  if (typeof argument !== "string" || !argument.startsWith("--") || argument.length === 2) fail(`unknown option ${argument}`);
  const key = argument.slice(2);
  if (!Object.hasOwn(options, key)) fail(`unknown option ${argument}`);
  if (options[key] !== null) fail(`${argument} was specified more than once`);
  const value = argv[index + 1];
  if (typeof value !== "string" || value.startsWith("-")) fail(`${argument} requires a value`);
  options[key] = value;
  return index + 2;
}

function parseArguments(argv) {
  const options = { receipt: null, dist: null, repo: null };
  let index = 0;
  while (index < argv.length) index = readOption(argv, index, options);
  for (const key of Object.keys(options)) if (options[key] === null) fail(`--${key} requires a value`);
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(options.repo)) fail("--repo must be OWNER/REPO");
  return { receipt: path.resolve(options.receipt), dist: path.resolve(options.dist), repo: options.repo };
}

function assertDirectory(directory) {
  const stats = fs.lstatSync(directory);
  if (stats.isSymbolicLink() || !stats.isDirectory()) fail("--dist must be a regular directory, not a symlink");
}

function assertRegularFile(file, label) {
  const stats = fs.lstatSync(file);
  if (stats.isSymbolicLink() || !stats.isFile()) fail(`${label} must be a regular file, not a symlink`);
}

function assertAssetSet(directory, assets) {
  assertDirectory(directory);
  const actual = fs.readdirSync(directory).sort();
  const expected = [...assets].sort();
  if (actual.length !== expected.length || actual.some((name, index) => name !== expected[index])) fail("--dist must contain exactly the eight expected release assets");
  for (const name of assets) assertRegularFile(path.join(directory, name), `asset ${name}`);
}

function assertHashable(stats, label) {
  if (!stats.isFile()) fail(label + " must be a regular file");
  if (!Number.isSafeInteger(stats.size) || stats.size > MAX_ASSET_BYTES) fail(label + " exceeds the bounded hash size");
}

async function digestStream(handle, policy) {
  const stream = handle.createReadStream({ autoClose: false, highWaterMark: HASH_CHUNK_BYTES });
  const digest = crypto.createHash("sha256");
  let bytesRead = 0;
  const timer = setTimeout(() => stream.destroy(new Error("asset hashing exceeded the operation deadline")), remainingMs(policy));
  try {
    for await (const chunk of stream) {
      remainingMs(policy);
      bytesRead += chunk.byteLength;
      if (bytesRead > MAX_ASSET_BYTES) fail("asset exceeds the bounded hash size");
      digest.update(chunk);
    }
    remainingMs(policy);
    return digest.digest("hex");
  } finally {
    clearTimeout(timer);
    if (!stream.readableEnded && !stream.destroyed) stream.destroy();
  }
}

async function hashRegularFile(file, label, policy) {
  const handle = await fs.promises.open(file, READ_FLAGS);
  try {
    const stats = await handle.stat();
    assertHashable(stats, label);
    const digest = await digestStream(handle, policy);
    const finalStats = await handle.stat();
    assertHashable(finalStats, label);
    if (finalStats.size !== stats.size) fail(`${label} changed while hashing`);
    return digest;
  } finally {
    try {
      await handle.close();
    } catch (error) {
      if (error.code !== "EBADF") throw error;
    }
  }
}

function expectedDigestMap(receipt, assets) {
  const archives = receipt.identity.archives;
  if (archives.length !== assets.length || assets.some((name) => !archives.some((archive) => archive.name === name))) fail("receipt identity archives must exactly match the GitHub asset set");
  return new Map(archives.map((archive) => [archive.name, archive.sha256]));
}

async function hashLocalAssets(receipt, directory, assets, policy) {
  assertAssetSet(directory, assets);
  const expected = expectedDigestMap(receipt, assets);
  for (const name of assets) {
    const digest = await hashRegularFile(path.join(directory, name), `asset ${name}`, policy);
    if (digest !== expected.get(name)) fail(`asset ${name} does not match the receipt digest`);
  }
}

function commandEnvironment(token) {
  const inherited = ["PATH", "HOME", "TMPDIR", "LANG", "LC_ALL"]
    .map((name) => [name, process.env[name]])
    .filter(([, value]) => value);
  return Object.fromEntries([["GH_TOKEN", token], ["GH_HOST", "github.com"], ...inherited]);
}

function isNotFound(error) {
  if (error?.status === 404 || error?.statusCode === 404) return true;
  const text = [error?.stderr, error?.stdout, error?.message].filter(Boolean).join("\n");
  return /(?:HTTP\s*404\b|status(?:\s+code)?\s*[:=]?\s*404\b|404\s+Not\s+Found)/i.test(text);
}

function parseJson(output, label) {
  try {
    return JSON.parse(output);
  } catch {
    fail(`${label} returned malformed JSON`);
  }
}

function versionFromTag(tag, label) {
  if (typeof tag !== "string" || !tag.startsWith("v")) fail(`${label} must be a v<semver> tag`);
  const version = tag.slice(1);
  try {
    compareReleaseTags(`v${version}`, `v${version}`);
  } catch {
    fail(`${label} must be a valid repository semantic version`);
  }
  return version;
}

function parseLatestMetadata(output) {
  const metadata = parseJson(output, "latest release probe");
  if (metadata === null || typeof metadata !== "object" || Array.isArray(metadata)) fail("latest release probe returned malformed metadata");
  if (typeof metadata.tag_name !== "string" || typeof metadata.draft !== "boolean" || typeof metadata.prerelease !== "boolean") fail("latest release probe returned malformed status metadata");
  if (metadata.draft || metadata.prerelease) fail("latest release is not a public stable release");
  return { state: "present", version: versionFromTag(metadata.tag_name, "latest release tag") };
}

function assertExactAssets(names, assets, label) {
  const expected = new Set(assets);
  const actual = new Set(names);
  if (names.length !== assets.length || actual.size !== names.length || actual.size !== expected.size || [...expected].some((name) => !actual.has(name))) {
    fail(`${label} does not contain the exact expected asset set`);
  }
}

function assertReleaseShape(metadata) {
  if (metadata === null || typeof metadata !== "object" || Array.isArray(metadata)) fail("release view returned malformed metadata");
  if (!Array.isArray(metadata.assets)) fail("release view returned malformed assets");
  return metadata;
}

function assertReleaseStatus(metadata, request, stable) {
  const checked = assertReleaseShape(metadata);
  if (checked.tagName !== request.tag || typeof checked.isDraft !== "boolean" || typeof checked.isPrerelease !== "boolean") fail("release view returned malformed status metadata");
  if (checked.isDraft) fail("release " + request.tag + " is a draft");
  if (stable && checked.isPrerelease) fail("release " + request.tag + " is not public stable");
}

function releaseAssetNames(metadata, assets) {
  const names = metadata.assets.map((asset) => asset?.name);
  if (names.some((name) => typeof name !== "string")) fail("release view returned a malformed asset name");
  assertExactAssets(names, assets, "release view assets");
  return names;
}

function parseReleaseView(output, request, assets, stable) {
  const metadata = parseJson(output, "release view");
  assertReleaseStatus(metadata, request, stable);
  return { isPrerelease: metadata.isPrerelease, assets: releaseAssetNames(metadata, assets) };
}

async function verifyRemoteAssets(context) {
  const { options, request, assets, expectedDigests, runGh } = context;
  const root = await fs.promises.mkdtemp(path.join(os.tmpdir(), "hardgate-github-promote-"));
  try {
    for (const name of assets) {
      const directory = await fs.promises.mkdtemp(path.join(root, "asset-"));
      await runGh(["release", "download", request.tag, "--repo", options.repo, "--pattern", name, "--dir", directory]);
      const entries = fs.readdirSync(directory);
      if (entries.length !== 1 || entries[0] !== name) fail(`downloaded asset ${name} was not an exact regular file`);
      const downloaded = path.join(directory, name);
      assertRegularFile(downloaded, `downloaded asset ${name}`);
      const digest = await hashRegularFile(downloaded, `downloaded asset ${name}`, request.policy);
      if (digest !== expectedDigests.get(name)) fail(`remote asset ${name} does not match the receipt digest`);
    }
  } finally {
    await fs.promises.rm(root, { recursive: true, force: true });
  }
}

function buildOperations(context) {
  const { options, request, assets, expectedDigests, token } = context;
  const environment = commandEnvironment(token);
  const runGh = async (argumentsList) => {
    const output = await runReleaseProcess("gh", argumentsList, {
      cwd: projectRoot,
      env: environment,
      maxBuffer: 4 * 1024 * 1024,
      timeoutMs: Math.min(SUBPROCESS_TIMEOUT_MS, remainingMs(request.policy)),
    });
    remainingMs(request.policy);
    return output;
  };
  const probe = async () => {
    try {
      return parseLatestMetadata(await runGh(["api", "-X", "GET", `repos/${options.repo}/releases/latest`]));
    } catch (error) {
      if (isNotFound(error)) return { state: "missing" };
      throw new Error("latest release probe failed");
    }
  };
  const inspect = async (stable) => {
    const output = await runGh(["release", "view", request.tag, "--repo", options.repo, "--json", "tagName,isDraft,isPrerelease,assets"]);
    parseReleaseView(output, request, assets, stable);
    await verifyRemoteAssets({ options, request, assets, expectedDigests, runGh });
    return true;
  };
  return {
    probe,
    verifyImmutable: async () => inspect(false),
    verifyDefault: async () => {
      const latest = await probe();
      if (latest.state !== "present" || latest.version !== request.version) fail("latest release is not the requested version");
      return inspect(true);
    },
    promote: async () => runGh(["release", "edit", request.tag, "--repo", options.repo, "--prerelease=false", "--latest=true"]),
  };
}

function promotionPolicy() {
  return { attempts: 3, delayMs: 250, deadline: performance.now() + OVERALL_TIMEOUT_MS };
}

function checkpointFailure(receiptPath, receipt) {
  try {
    recordFailure(receipt, { channel: GITHUB_CHANNEL, code: EXPECTED_FAILURE_CODE, message: EXPECTED_FAILURE_MESSAGE });
    writeReceiptAtomicSync(receiptPath, receipt, receipt.identity);
  } catch {
    // Preserve the original operation error when a failure checkpoint cannot be written.
  }
}

async function runPromotion(options, operationsFactory = buildOperations) {
  const receiptPath = path.resolve(options.receipt);
  let receipt;
  try {
    receipt = readReceipt(receiptPath);
    validateGithubPromotionReceipt(receipt);
    const policy = promotionPolicy();
    const assets = expectedGithubAssets(receipt.identity.version);
    const expectedDigests = expectedDigestMap(receipt, assets);
    await hashLocalAssets(receipt, options.dist, assets, policy);
    const token = process.env.GH_TOKEN;
    if (!token) fail("GH_TOKEN is required");
    const initialState = receipt.channels[GITHUB_CHANNEL].state;
    const request = { version: receipt.identity.version, tag: `v${receipt.identity.version}`, policy };
    const operations = operationsFactory({ options, request, assets, expectedDigests, token });
    const result = await promoteGithubChannel({ receipt, policy }, operations);
    if (initialState === "exact_consumer_verified") writeReceiptAtomicSync(receiptPath, receipt, receipt.identity);
    return result;
  } catch (error) {
    if (receipt) checkpointFailure(receiptPath, receipt);
    throw error;
  }
}

async function main(argv) {
  const options = parseArguments(argv);
  const result = await runPromotion(options);
  console.log(["github channel:", result.publication, `(${result.state})`].join(" "));
}

function reportFailure() {
  process.stderr.write(`${EXPECTED_FAILURE_CODE}: ${EXPECTED_FAILURE_MESSAGE}\n`);
  process.exitCode = 1;
}

const invokedPath = process.argv[1] ? path.resolve(process.argv[1]) : "";
if (invokedPath === fileURLToPath(import.meta.url)) main(process.argv.slice(2)).catch(reportFailure);
