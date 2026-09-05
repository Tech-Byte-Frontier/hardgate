#!/usr/bin/env node
// Stage exact release assets through the installed GitHub CLI.
"use strict";

import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";
import { runReleaseProcess } from "./release-process.mjs";
import { projectRoot } from "./release-support.mjs";
import { expectedGithubAssets, remainingMs, stageGithubRelease } from "./github-staging-state.mjs";

const SUBPROCESS_TIMEOUT_MS = 60_000;
const OVERALL_TIMEOUT_MS = 20 * 60 * 1000;
const MAX_ASSET_BYTES = 1024 * 1024 * 1024;
const HASH_CHUNK_BYTES = 64 * 1024;
const READ_FLAGS = fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW ?? 0);

function fail(message) {
  throw new Error(`stage-github-release: ${message}`);
}

function readCliOption(options, argv, index) {
  const argument = argv[index];
  const optionName = argument.slice(2);
  const nextIndex = index + 1;
  if (!Object.prototype.hasOwnProperty.call(options, optionName)) fail(`unknown option ${argument}`);
  if (options[optionName] !== null) fail(`${argument} was specified more than once`);
  const value = argv[nextIndex];
  if (!value || value.startsWith("-")) fail(`${argument} requires a value`);
  options[optionName] = value;
  return nextIndex;
}

function parseArguments(argv) {
  const options = { repo: null, tag: null, version: null, dist: null };
  for (let index = 0; index < argv.length; index += 1) {
    if (!argv[index].startsWith("--")) fail(`unknown option ${argv[index]}`);
    index = readCliOption(options, argv, index);
  }
  for (const key of Object.keys(options)) if (options[key] === null) fail(`--${key} requires a value`);
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(options.repo)) fail("--repo must be OWNER/REPO");
  if (!/^v/.test(options.tag)) fail("--tag must be v<semver>");
  if (!/^v/.test(options.tag) || options.tag !== `v${options.version}`) fail("--tag and --version must identify the same release");
  return { ...options, dist: path.resolve(options.dist) };
}

function assertRegularDirectory(directory) {
  const stats = fs.lstatSync(directory);
  if (stats.isSymbolicLink() || !stats.isDirectory()) fail("--dist must be a regular directory, not a symlink");
}

function validateDist(directory, assets) {
  assertRegularDirectory(directory);
  const expected = [...assets].sort();
  const actual = fs.readdirSync(directory).sort();
  if (actual.length !== expected.length || actual.some((name, index) => name !== expected[index])) fail("--dist must contain exactly the eight expected release assets");
  for (const name of assets) {
    const stats = fs.lstatSync(path.join(directory, name));
    if (stats.isSymbolicLink() || !stats.isFile()) fail(`asset ${name} must be a regular file, not a symlink`);
  }
}

function commandEnvironment(token) {
  const environment = { GH_TOKEN: token, GH_HOST: "github.com" };
  for (const name of ["PATH", "HOME", "TMPDIR", "LANG", "LC_ALL"]) {
    if (process.env[name]) environment[name] = process.env[name];
  }
  return environment;
}

function explicitReleaseNotFound(error) {
  const output = [error?.stderr, error?.stdout, error?.message].filter(Boolean).join("\n");
  return /release\s+not\s+found|HTTP\s*404|404\s+Not\s+Found/i.test(output);
}

function parseReleaseMetadata(output) {
  let metadata;
  try {
    metadata = JSON.parse(output);
  } catch {
    fail("gh release view returned malformed JSON");
  }
  if (metadata === null || typeof metadata !== "object" || !Array.isArray(metadata.assets)) fail("gh release view returned malformed metadata");
  return {
    state: "present",
    tag: metadata.tagName,
    isDraft: metadata.isDraft,
    isPrerelease: metadata.isPrerelease,
    assets: metadata.assets.map((asset) => asset?.name),
  };
}

function hashStream(stream, policy) {
  return new Promise((resolve, reject) => {
    const hash = crypto.createHash("sha256");
    let settled = false;
    let timer;
    const finish = (error, digest) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (error) reject(error);
      else resolve(digest);
    };
    try {
      timer = setTimeout(() => {
        const error = new Error("asset hash deadline exhausted");
        stream.destroy(error);
        finish(error);
      }, remainingMs(policy));
    } catch (error) {
      finish(error);
      return;
    }
    stream.on("data", (chunk) => {
      try {
        remainingMs(policy);
        hash.update(chunk);
      } catch (error) {
        stream.destroy(error);
        finish(error);
      }
    });
    stream.once("error", (error) => finish(error));
    stream.once("end", () => {
      try {
        remainingMs(policy);
        finish(null, hash.digest("hex"));
      } catch (error) {
        finish(error);
      }
    });
  });
}

async function sha256File(filePath, policy) {
  const handle = await fs.promises.open(filePath, READ_FLAGS);
  let stream;
  try {
    const stats = await handle.stat();
    if (!stats.isFile()) fail(`asset ${path.basename(filePath)} is not a regular file`);
    if (stats.size > MAX_ASSET_BYTES) fail(`asset ${path.basename(filePath)} exceeds the maximum size`);
    const options = { fd: handle.fd, autoClose: false, highWaterMark: HASH_CHUNK_BYTES };
    if (stats.size > 0) options.end = stats.size - 1;
    stream = fs.createReadStream(null, options);
    const digest = await hashStream(stream, policy);
    const finalStats = await handle.stat();
    if (finalStats.size !== stats.size) fail(`asset ${path.basename(filePath)} changed while hashing`);
    return digest;
  } finally {
    if (stream && !stream.readableEnded && !stream.destroyed) stream.destroy();
    try {
      await handle.close();
    } catch (error) {
      if (error.code !== "EBADF") throw error;
    }
  }
}

async function compareFiles(left, right, policy) {
  const leftDigest = await sha256File(left, policy);
  const rightDigest = await sha256File(right, policy);
  if (leftDigest !== rightDigest) fail(`remote asset bytes mismatch for ${path.basename(left)}`);
}

function assertDownloadedFile(filePath, name) {
  const stats = fs.lstatSync(filePath);
  if (stats.isSymbolicLink() || !stats.isFile()) fail(`downloaded asset ${name} is not a regular file`);
}

async function buildOperations(options, request, token) {
  const environment = commandEnvironment(token);
  const runGh = async (argumentsList) => runReleaseProcess("gh", argumentsList, {
    cwd: projectRoot,
    env: environment,
    maxBuffer: 4 * 1024 * 1024,
    timeoutMs: Math.min(SUBPROCESS_TIMEOUT_MS, remainingMs(request.policy)),
  });
  return {
    probe: async () => {
      try {
        const output = await runGh(["release", "view", request.tag, "--repo", request.repo, "--json", "tagName,isDraft,isPrerelease,assets"]);
        return parseReleaseMetadata(output);
      } catch (error) {
        if (explicitReleaseNotFound(error)) return { state: "missing" };
        throw new Error(`release probe failed: ${error.message}`);
      }
    },
    create: async () => {
      const paths = request.assets.map((name) => path.join(options.dist, name));
      await runGh(["release", "create", request.tag, ...paths, "--repo", request.repo, "--verify-tag", "--generate-notes", "--prerelease", "--latest=false"]);
    },
    upload: async (_request, name) => {
      await runGh(["release", "upload", request.tag, path.join(options.dist, name), "--repo", request.repo]);
    },
    verify: async (_request, names) => {
      const directory = await fs.promises.mkdtemp(path.join(os.tmpdir(), "hardgate-github-stage-"));
      try {
        for (const name of names) {
          await runGh(["release", "download", request.tag, "--repo", request.repo, "--pattern", name, "--dir", directory]);
          const downloaded = path.join(directory, name);
          assertDownloadedFile(downloaded, name);
          await compareFiles(path.join(options.dist, name), downloaded, request.policy);
        }
      } finally {
        await fs.promises.rm(directory, { recursive: true, force: true });
      }
    },
  };
}

async function main(argv) {
  const options = parseArguments(argv);
  const assets = expectedGithubAssets(options.version);
  validateDist(options.dist, assets);
  const token = process.env.GH_TOKEN;
  if (!token) fail("GH_TOKEN is required");
  const request = { repo: options.repo, tag: options.tag, version: options.version, assets, policy: { deadline: performance.now() + OVERALL_TIMEOUT_MS } };
  const operations = await buildOperations(options, request, token);
  const result = await stageGithubRelease(request, operations);
  console.log(`github release: ${result.publication} (${result.state}; prerelease=${result.prerelease})`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((error) => {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
