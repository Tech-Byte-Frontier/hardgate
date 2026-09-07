#!/usr/bin/env node
// Build reproducible native release archives from binaries downloaded by CI.
// Usage: node scripts/release-package.mjs --incoming build-binaries --output dist
"use strict";

import { NATIVE_PACKAGES, executableName } from "./release-platforms.mjs";

import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { option } from "./release-support.mjs";

const targets = Object.values(NATIVE_PACKAGES).map(({ target, name }) => [target, name]);

function fail(message) {
  throw new Error(`release-package: ${message}`);
}

function run(command, args) {
  const result = spawnSync(command, args, { encoding: "utf8", maxBuffer: 1024 * 1024 });
  if (result.status !== 0) fail(`${command} failed: ${result.error?.message ?? result.stderr}`);
  return result.stdout;
}

function locate(incoming, target, packageName) {
  const prefix = `binary-${target}-attempt-`;
  const attempts = fs.readdirSync(incoming)
    .filter((name) => name.startsWith(prefix) && /^[1-9][0-9]*$/.test(name.slice(prefix.length)))
    .sort((a, b) => Number(b.slice(prefix.length)) - Number(a.slice(prefix.length)));
  const names = [...attempts, `binary-${target}`, target];
  for (const name of names) {
    for (const candidate of [path.join(incoming, name, executableName(packageName)), path.join(incoming, name, "bin", executableName(packageName))]) {
      if (fs.existsSync(candidate)) return candidate;
    }
  }
  fail(`missing binary for ${target} under ${incoming}`);
}

function archiveBinary({ binary, output, packageName, target, version, commit, staging }) {
  const packageRoot = path.join(staging, packageName);
  fs.mkdirSync(packageRoot, { recursive: true });
  // Explicit modes make the tar stream independent of the caller's umask.
  // The package root and every archived member have a deliberate mode below.
  fs.chmodSync(packageRoot, 0o755);
  const destination = path.join(packageRoot, executableName(packageName));
  fs.copyFileSync(binary, destination);
  fs.chmodSync(destination, 0o755);
  const metadata = {
    name: "hardgate",
    version,
    target,
    package: packageName,
    commit,
  };
  const metadataPath = path.join(packageRoot, "BUILD-METADATA.json");
  fs.writeFileSync(metadataPath, `${JSON.stringify(metadata)}\n`);
  fs.chmodSync(metadataPath, 0o644);

  const tarPath = path.join(output, `${packageName}.tar`);
  const archivePath = path.join(output, `${packageName}.tar.gz`);
  run("tar", [
    "--format=ustar",
    "--sort=name",
    "--mtime=@0",
    "--owner=0",
    "--group=0",
    "--numeric-owner",
    "-cf",
    tarPath,
    "-C",
    staging,
    packageName,
  ]);
  const compressed = spawnSync("gzip", ["-n", "-c", tarPath], { encoding: null, maxBuffer: 64 * 1024 * 1024 });
  if (compressed.status !== 0) fail(`gzip failed for ${packageName}: ${compressed.error?.message ?? compressed.stderr}`);
  fs.writeFileSync(archivePath, compressed.stdout);
  fs.rmSync(tarPath, { force: true });
  return archivePath;
}

function checksum(file) {
  return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}

const incoming = path.resolve(option("--incoming", "build-binaries"));
const output = path.resolve(option("--output", "dist"));
const version = option("--version");
const commit = option("--commit");
if (!version || !commit) fail("--version and --commit are required");
if (commit === "unknown" || !/^[0-9a-f]{40}(?:[0-9a-f]{24})?$/i.test(commit)) {
  fail(`commit must be a full hexadecimal source identity, got ${commit}`);
}
fs.mkdirSync(output, { recursive: true });
const staging = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-release-"));
const archives = [];
try {
  for (const [target, pkg] of targets) {
    archives.push(archiveBinary({
      binary: locate(incoming, target, pkg),
      output,
      packageName: pkg,
      target,
      version,
      commit,
      staging,
    }));
  }
  const lines = archives.map((file) => `${checksum(file)}  ${path.basename(file)}`);
  fs.writeFileSync(path.join(output, "SHA256SUMS"), `${lines.join("\n")}\n`);
  console.log(`release-package: wrote ${archives.length} reproducible archives and SHA256SUMS`);
} finally {
  fs.rmSync(staging, { recursive: true, force: true });
}
