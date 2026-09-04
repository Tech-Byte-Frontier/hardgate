#!/usr/bin/env node
// Verify published npm package bytes and platform metadata against the
// release archives before the wrapper package is published (or a channel is
// declared complete).
// Usage: node scripts/verify-npm-publication.mjs --version <version> --dist dist
"use strict";

import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { isRetryableNpmPackError } from "./npm-pack-retry.mjs";
import { archiveMemberMode, isExecutableMode, option, projectRoot as root, runCommand } from "./release-support.mjs";

const packages = [
  ["hardgate-linux-x64", ["linux"], ["x64"], ["glibc"]],
  ["hardgate-linux-x64-musl", ["linux"], ["x64"], ["musl"]],
  ["hardgate-linux-arm64", ["linux"], ["arm64"], ["glibc"]],
  ["hardgate-linux-arm64-musl", ["linux"], ["arm64"], ["musl"]],
  ["hardgate-darwin-x64", ["darwin"], ["x64"], undefined],
  ["hardgate-darwin-arm64", ["darwin"], ["arm64"], undefined],
];
const packageNames = packages.map(([name]) => name).sort();
const maxAttempts = Number.parseInt(process.env.NPM_VERIFY_ATTEMPTS ?? "20", 10);
const retryDelay = Number.parseInt(process.env.NPM_VERIFY_DELAY_SECONDS ?? "10", 10);

function fail(message) {
  throw new Error(`verify-npm-publication: ${message}`);
}

const run = (command, args, options = {}) => runCommand("verify-npm-publication", command, args, options);

function digest(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex");
}

function readTar(archive, member) {
  const result = spawnSync("tar", ["-xOzf", archive, member], { encoding: null, maxBuffer: 64 * 1024 * 1024 });
  if (result.status !== 0) fail(`${path.basename(archive)} lacks ${member}`);
  return result.stdout;
}

function unpackTar(archive, directory) {
  run("tar", ["-xzf", archive, "-C", directory]);
  const packageDirectory = path.join(directory, "package");
  if (!fs.existsSync(packageDirectory)) fail(`${path.basename(archive)} has no package/ root`);
  return packageDirectory;
}

function verifyExecutableMember(archive, packageDirectory, packageName) {
  const member = "package/bin/hardgate";
  const mode = archiveMemberMode(run("tar", ["-tvzf", archive]), member);
  if (!isExecutableMode(mode)) {
    fail(`${packageName} published package bin/hardgate must retain an executable mode`);
  }
  const extractedMode = fs.statSync(path.join(packageDirectory, "bin/hardgate")).mode;
  if ((extractedMode & 0o111) === 0) {
    fail(`${packageName} published package bin/hardgate is not executable after extraction`);
  }
}

function packOnce(spec, directory) {
  // Keep npm's error diagnostics so the bounded retry classifier can
  // distinguish registry propagation (for example E404 immediately after a
  // publish) from authentication or package-integrity failures. `--silent`
  // suppresses those diagnostics and turns every failure into an opaque exit
  // status that must fail closed.
  const result = spawnSync("npm", ["pack", spec, "--ignore-scripts", "--loglevel=error", "--pack-destination", directory], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 4 * 1024 * 1024,
    env: { ...process.env, npm_config_audit: "false", npm_config_fund: "false" },
  });
  if (result.status !== 0) {
    const detail = [result.error?.code, result.error?.message, result.stderr, result.stdout]
      .filter(Boolean)
      .join("\n");
    throw new Error(detail || `npm pack exited with status ${result.status}`);
  }
  const archives = fs.readdirSync(directory).filter((name) => name.endsWith(".tgz"));
  if (archives.length !== 1) throw new Error(`npm pack ${spec} produced ${archives.length} tarballs`);
  return path.join(directory, archives[0]);
}

function packWithRetry(spec) {
  let lastError;
  for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-npm-pack-"));
    try {
      return { archive: packOnce(spec, directory), directory };
    } catch (error) {
      lastError = error;
      fs.rmSync(directory, { recursive: true, force: true });
      if (!isRetryableNpmPackError(error)) {
        fail(`npm pack ${spec} failed without retry: ${error?.message ?? error}`);
      }
      if (attempt < maxAttempts && retryDelay > 0) spawnSync("sleep", [String(retryDelay)]);
    }
  }
  fail(`npm pack ${spec} failed after ${maxAttempts} bounded attempts: ${lastError?.message ?? lastError}`);
}

function assertArray(manifest, key, expected, packageName) {
  const actual = manifest[key];
  if (JSON.stringify(actual ?? undefined) !== JSON.stringify(expected)) {
    fail(`${packageName} manifest ${key}=${JSON.stringify(actual)} expected ${JSON.stringify(expected)}`);
  }
}

function verifyPlatformPackage(version, dist, [name, osValues, cpuValues, libcValues]) {
  const packed = packWithRetry(`${name}@=${version}`);
  try {
    const packageDirectory = unpackTar(packed.archive, packed.directory);
    verifyExecutableMember(packed.archive, packageDirectory, name);
    const manifestPath = path.join(packageDirectory, "package.json");
    const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
    if (manifest.name !== name || manifest.version !== version) {
      fail(`${name} manifest identity is ${manifest.name}@${manifest.version}, expected ${name}@${version}`);
    }
    assertArray(manifest, "os", osValues, name);
    assertArray(manifest, "cpu", cpuValues, name);
    if (libcValues) assertArray(manifest, "libc", libcValues, name);
    else if (Object.hasOwn(manifest, "libc")) fail(`${name} must not advertise a libc constraint`);
    const publishedBinary = fs.readFileSync(path.join(packageDirectory, "bin/hardgate"));
    const releaseBinary = readTar(path.join(dist, `${name}.tar.gz`), `${name}/hardgate`);
    if (digest(publishedBinary) !== digest(releaseBinary) || !publishedBinary.equals(releaseBinary)) {
      fail(`${name} npm binary does not byte-match its verified release archive`);
    }
    console.log(`${name}@${version}: manifest and SHA256 binary match verified archive`);
  } finally {
    fs.rmSync(packed.directory, { recursive: true, force: true });
  }
}

function verifyWrapper(version) {
  const packed = packWithRetry(`@tech-byte-frontier/hardgate@=${version}`);
  try {
    const packageDirectory = unpackTar(packed.archive, packed.directory);
    const manifest = JSON.parse(fs.readFileSync(path.join(packageDirectory, "package.json"), "utf8"));
    if (manifest.name !== "@tech-byte-frontier/hardgate" || manifest.version !== version) {
      fail(`wrapper manifest identity is ${manifest.name}@${manifest.version}, expected @tech-byte-frontier/hardgate@${version}`);
    }
    const optional = Object.keys(manifest.optionalDependencies ?? {}).sort();
    if (optional.join("\n") !== packageNames.join("\n")) {
      fail(`wrapper optionalDependencies must contain exactly ${packageNames.join(", ")}`);
    }
    for (const name of packageNames) {
      if (manifest.optionalDependencies[name] !== version) {
        fail(`wrapper optionalDependencies[${name}] must be ${version}`);
      }
    }
    const publishedLauncher = fs.readFileSync(path.join(packageDirectory, "bin/hardgate.js"));
    const sourceLauncher = fs.readFileSync(path.join(root, "npm/hardgate/bin/hardgate.js"));
    if (digest(publishedLauncher) !== digest(sourceLauncher) || !publishedLauncher.equals(sourceLauncher)) {
      fail("published wrapper launcher does not byte-match the checked-out tag");
    }
    console.log(`@tech-byte-frontier/hardgate@${version}: manifest and launcher match checked-out tag`);
  } finally {
    fs.rmSync(packed.directory, { recursive: true, force: true });
  }
}

const version = option("--version");
const dist = path.resolve(option("--dist", "dist"));
const platformOnly = process.argv.includes("--platform-only");
const selectedPackage = option("--package");
if (!version) fail("--version is required");
if (process.argv.includes("--package") && !selectedPackage) fail("--package requires a platform package name");
if (selectedPackage && !packageNames.includes(selectedPackage)) {
  fail(`--package must identify one of the six platform packages, got ${selectedPackage}`);
}
const platformsToVerify = selectedPackage
  ? packages.filter(([name]) => name === selectedPackage)
  : packages;
for (const platform of platformsToVerify) verifyPlatformPackage(version, dist, platform);
if (!platformOnly) verifyWrapper(version);
const platformLabel = selectedPackage ? selectedPackage : "all six platform packages";
console.log(`verify-npm-publication: ${platformLabel}${platformOnly ? "" : " and wrapper"} verified at ${version}`);
