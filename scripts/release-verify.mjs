#!/usr/bin/env node
// Verify release archives before any registry or GitHub publication.
// Usage: node scripts/release-verify.mjs --dist dist --version <version> --commit <sha> --tag v<version>
"use strict";

import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { classifyBinaryAbi } from "./release-abi.mjs";
import { archiveMemberMode, isExecutableMode, option, runCommand } from "./release-support.mjs";

const targets = [
  ["x86_64-unknown-linux-gnu", "hardgate-linux-x64", /x86-64/, "gnu"],
  ["x86_64-unknown-linux-musl", "hardgate-linux-x64-musl", /x86-64/, "musl"],
  ["aarch64-unknown-linux-gnu", "hardgate-linux-arm64", /ARM aarch64/, "gnu"],
  ["aarch64-unknown-linux-musl", "hardgate-linux-arm64-musl", /ARM aarch64/, "musl"],
  ["x86_64-apple-darwin", "hardgate-darwin-x64", /x86_64/, null],
  ["aarch64-apple-darwin", "hardgate-darwin-arm64", /arm64/, null],
];

function fail(message) {
  throw new Error(`release-verify: ${message}`);
}

const run = (command, args, options = {}) => runCommand("release-verify", command, args, options);

function digest(file) {
  return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}

const MAX_BINARY_BYTES = 128 * 1024 * 1024;

function verifyEmbeddedIdentity({ binaryPath, version, commit, target, packageName }) {
  const { size } = fs.statSync(binaryPath);
  if (!Number.isSafeInteger(size) || size > MAX_BINARY_BYTES) {
    fail(`${packageName} binary is unreasonably large for bounded identity verification`);
  }
  const bytes = fs.readFileSync(binaryPath);
  const marker = Buffer.from(`${version} (${commit})`, "utf8");
  const targetMarker = Buffer.from(`hardgate-target:${target}`, "utf8");
  if (!bytes.includes(Buffer.from(version, "utf8")) || !bytes.includes(Buffer.from(commit, "utf8"))) {
    fail(`${packageName} binary does not embed the expected version and source commit`);
  }
  if (!bytes.includes(marker)) {
    fail(`${packageName} binary does not embed the expected version/commit identity`);
  }
  if (!bytes.includes(targetMarker)) {
    fail(`${packageName} binary does not embed the expected Cargo target marker ${target}`);
  }
}

function verifyTagIdentity(tag, commit) {
  if (!tag) return;
  const tagged = run("git", ["rev-parse", `${tag}^{commit}`]).trim();
  const head = run("git", ["rev-parse", "HEAD"]).trim();
  if (tagged !== commit || head !== commit) fail(`tag ${tag} does not resolve to checked-out commit ${commit}`);
}

function verifyChecksums(dist, expectedNames) {
  const file = path.join(dist, "SHA256SUMS");
  if (!fs.existsSync(file)) fail("SHA256SUMS is missing");
  const archiveNames = fs.readdirSync(dist).filter((name) => name.endsWith(".tar.gz")).sort();
  const expectedArchives = expectedNames.filter((name) => name.endsWith(".tar.gz"));
  if (archiveNames.join("\n") !== [...expectedArchives].sort().join("\n")) {
    fail(`dist must contain exactly ${expectedArchives.join(", ")}`);
  }
  const lines = fs.readFileSync(file, "utf8").trim().split("\n").filter(Boolean);
  const names = lines.map((line) => line.match(/^[0-9a-f]{64}  (.+)$/i)?.[1]).filter(Boolean);
  if (names.length !== expectedNames.length || names.some((name, i) => name !== expectedNames[i])) {
    fail(`SHA256SUMS must contain exactly ${expectedNames.join(", ")}`);
  }
  for (const line of lines) {
    const [, expected, name] = line.match(/^([0-9a-f]{64})  (.+)$/i) ?? [];
    if (!expected || !name) fail(`malformed SHA256SUMS entry: ${line}`);
    const actual = digest(path.join(dist, name));
    if (expected.toLowerCase() !== actual) fail(`checksum mismatch for ${name}`);
  }
}

function extract(archive, member, directory) {
  const output = path.join(directory, member.replaceAll("/", "_"));
  const bytes = spawnSync("tar", ["-xOzf", archive, member], {
    encoding: null,
    maxBuffer: 64 * 1024 * 1024,
  });
  if (bytes.status !== 0) fail(`archive ${path.basename(archive)} lacks ${member}`);
  fs.writeFileSync(output, bytes.stdout);
  return output;
}

function verifyExecutableMember(archive, packageName) {
  const member = `${packageName}/hardgate`;
  const mode = archiveMemberMode(run("tar", ["-tvzf", archive]), member);
  if (!isExecutableMode(mode)) {
    fail(`${packageName} archive member hardgate must retain an executable mode before extraction`);
  }
}

function verifyBinaryAbi(binaryPath, target, abi, packageName) {
  if (!abi) return;
  const report = run("file", ["-b", binaryPath]);
  const programHeaders = run("readelf", ["-l", binaryPath]);
  const symbols = run("readelf", ["-sW", binaryPath]);
  const notes = run("readelf", ["-n", binaryPath]);
  const evidence = classifyBinaryAbi({
    report,
    programHeaders,
    symbols,
    notes,
    abi,
    // verifyEmbeddedIdentity already rejected a missing exact marker. This
    // flag lets the classifier preserve stripped static musl binaries while
    // still requiring positive target evidence rather than absence of glibc.
    targetMarkerValid: abi === "musl" && target.endsWith("-musl"),
  });
  if (!evidence.ok) {
    fail(`${packageName} ${abi} target ${target} ABI evidence failed: ${evidence.reason}`);
  }
}

function verifyArchive({ dist, target, pkg, archPattern, abi, version, commit, directory }) {
  const archive = path.join(dist, `${pkg}.tar.gz`);
  if (!fs.existsSync(archive)) fail(`missing ${path.basename(archive)}`);
  verifyExecutableMember(archive, pkg);
  const metadataPath = extract(archive, `${pkg}/BUILD-METADATA.json`, directory);
  let metadata;
  try {
    metadata = JSON.parse(fs.readFileSync(metadataPath, "utf8"));
  } catch (error) {
    fail(`${pkg} metadata is not valid JSON: ${error.message}`);
  }
  for (const [key, value] of Object.entries({ name: "hardgate", version, target, package: pkg, commit })) {
    if (metadata[key] !== value) fail(`${pkg} metadata ${key} is ${metadata[key] ?? "<missing>"}`);
  }
  const binaryPath = extract(archive, `${pkg}/hardgate`, directory);
  // tar -xO writes bytes without preserving the executable bit. Restore it
  // before the host smoke test and keep the extracted file bounded for the
  // embedded identity check below.
  fs.chmodSync(binaryPath, 0o755);
  verifyEmbeddedIdentity({ binaryPath, version, commit, target, packageName: pkg });
  const report = run("file", ["-b", binaryPath]);
  if (!archPattern.test(report)) fail(`${pkg} architecture does not match ${target}: ${report.trim()}`);
  verifyBinaryAbi(binaryPath, target, abi, pkg);
  const listing = run("tar", ["-tzf", archive]).split("\n").filter(Boolean).sort();
  const expected = [`${pkg}/`, `${pkg}/BUILD-METADATA.json`, `${pkg}/hardgate`];
  if (listing.join("\n") !== expected.join("\n")) fail(`${pkg} contains unexpected archive members`);
  return { archive, binaryPath };
}

function hostTarget() {
  if (process.platform === "darwin" && process.arch === "arm64") return "hardgate-darwin-arm64";
  if (process.platform === "darwin" && process.arch === "x64") return "hardgate-darwin-x64";
  if (process.platform === "linux" && process.arch === "arm64") return "hardgate-linux-arm64";
  if (process.platform === "linux" && process.arch === "x64") return "hardgate-linux-x64";
  return null;
}

const dist = path.resolve(option("--dist", "dist"));
const version = option("--version");
const commit = option("--commit");
const tag = option("--tag");
if (!version || !commit) fail("--version and --commit are required");
if (commit === "unknown" || !/^[0-9a-f]{40}(?:[0-9a-f]{24})?$/i.test(commit)) {
  fail(`commit must be a full hexadecimal source identity, got ${commit}`);
}
if (tag && tag !== `v${version}`) fail(`tag ${tag} does not identify version ${version}`);
verifyTagIdentity(tag, commit);
const names = targets.map(([, pkg]) => `${pkg}.tar.gz`);
verifyChecksums(dist, [...names, `hardgate-${version}.sbom.cdx.json`]);
const directory = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-verify-"));
try {
  const checked = targets.map(([target, pkg, pattern, abi]) => verifyArchive({
    dist,
    target,
    pkg,
    archPattern: pattern,
    abi,
    version,
    commit,
    directory,
  }));
  const host = hostTarget();
  const smoke = checked.find(({ archive }) => path.basename(archive, ".tar.gz") === host);
  if (smoke) {
    const result = spawnSync(smoke.binaryPath, ["--version"], { encoding: "utf8" });
    const expectedOutput = `hardgate ${version} (${commit})`;
    if (result.error || result.status !== 0 || result.stdout.trim() !== expectedOutput) {
      fail(`host binary smoke test failed: ${result.stderr ?? result.error?.message ?? result.stdout}`);
    }
  }
  console.log(`release-verify: ${checked.length} archives, checksums, metadata, architecture, and version verified`);
} finally {
  fs.rmSync(directory, { recursive: true, force: true });
}
