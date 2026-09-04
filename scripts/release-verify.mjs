#!/usr/bin/env node
// Verify release archives before any registry or GitHub publication.
// Usage: node scripts/release-verify.mjs --dist dist --version 0.4.3 --commit <sha> --tag v0.4.3
"use strict";

import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";

const targets = [
  ["x86_64-unknown-linux-gnu", "hardgate-linux-x64", /x86-64/],
  ["x86_64-unknown-linux-musl", "hardgate-linux-x64-musl", /x86-64/],
  ["aarch64-unknown-linux-gnu", "hardgate-linux-arm64", /ARM aarch64/],
  ["aarch64-unknown-linux-musl", "hardgate-linux-arm64-musl", /ARM aarch64/],
  ["x86_64-apple-darwin", "hardgate-darwin-x64", /x86_64/],
  ["aarch64-apple-darwin", "hardgate-darwin-arm64", /arm64/],
];

function option(name, fallback) {
  const index = process.argv.indexOf(name);
  if (index >= 0) return process.argv[index + 1];
  return process.argv.find((value) => value.startsWith(`${name}=`))?.slice(name.length + 1) ?? fallback;
}

function fail(message) {
  throw new Error(`release-verify: ${message}`);
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: "utf8", ...options });
  if (result.status !== 0) fail(`${command} failed: ${result.error?.message ?? result.stderr}`);
  return result.stdout;
}

function digest(file) {
  return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
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
  if (archiveNames.join("\n") !== [...expectedNames].sort().join("\n")) {
    fail(`dist must contain exactly ${expectedNames.join(", ")}`);
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

function verifyArchive(dist, target, pkg, archPattern, version, commit, directory) {
  const archive = path.join(dist, `${pkg}.tar.gz`);
  if (!fs.existsSync(archive)) fail(`missing ${path.basename(archive)}`);
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
  const report = run("file", ["-b", binaryPath]);
  if (!archPattern.test(report)) fail(`${pkg} architecture does not match ${target}: ${report.trim()}`);
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
verifyChecksums(dist, names);
const directory = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-verify-"));
try {
  const checked = targets.map(([target, pkg, pattern]) => verifyArchive(dist, target, pkg, pattern, version, commit, directory));
  const host = hostTarget();
  const smoke = checked.find(({ archive }) => path.basename(archive, ".tar.gz") === host);
  if (smoke) {
    const result = spawnSync(smoke.binaryPath, ["--version"], { encoding: "utf8" });
    if (result.status !== 0 || !result.stdout.trim().startsWith(`hardgate ${version} (`)) {
      fail(`host binary smoke test failed: ${result.stderr ?? result.error?.message ?? result.stdout}`);
    }
  }
  console.log(`release-verify: ${checked.length} archives, checksums, metadata, architecture, and version verified`);
} finally {
  fs.rmSync(directory, { recursive: true, force: true });
}
