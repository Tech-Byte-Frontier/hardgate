// Exercise packed wrapper and optional-native archives through real npm and
// pnpm installs. No Cargo build or remote registry is involved.
"use strict";

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

import { checkPackedConsumers } from "../scripts/check-packed-consumers.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const fixtureRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-packed-consumer-test-"));

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: "utf8", timeout: 30_000, ...options });
  assert.equal(result.error, undefined, `${command} could not start: ${result.error?.message ?? "unknown error"}`);
  assert.equal(result.status, 0, `${command} ${args.join(" ")} failed\n${result.stdout}\n${result.stderr}`);
  return result;
}

function hash(file) {
  return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}

function fixtureEnvironment() {
  const npmCache = path.join(fixtureRoot, "npm-cache");
  const home = path.join(fixtureRoot, "home");
  fs.mkdirSync(npmCache, { recursive: true });
  fs.mkdirSync(home, { recursive: true });
  return {
    ...process.env,
    HOME: home,
    npm_config_cache: npmCache,
    NPM_CONFIG_CACHE: npmCache,
    npm_config_audit: "false",
    npm_config_fund: "false",
    npm_config_update_notifier: "false",
  };
}

function makeFixtureArchives() {
  const packagesDir = path.join(fixtureRoot, "packages");
  fs.mkdirSync(packagesDir, { recursive: true });
  const nativeSource = path.join(fixtureRoot, "fixture.c");
  const nativeBinary = path.join(fixtureRoot, "fixture-native");
  fs.writeFileSync(nativeSource, `#include <stdio.h>\n#include <string.h>\nint main(int argc, char **argv) {\n  if (argc == 2 && strcmp(argv[1], "--version") == 0) puts("hardgate 0.5.0 (aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa)");\n  return 0;\n}\n`);
  run("cc", ["-O2", nativeSource, "-o", nativeBinary]);
  fs.chmodSync(nativeBinary, 0o755);
  const names = [
    "hardgate-linux-x64",
    "hardgate-linux-x64-musl",
    "hardgate-linux-arm64",
    "hardgate-linux-arm64-musl",
    "hardgate-darwin-x64",
    "hardgate-darwin-arm64",
  ];
  const packageDirectories = ["hardgate", ...names];
  for (const name of packageDirectories) {
    const packageDirectory = path.join(fixtureRoot, name);
    fs.cpSync(path.join(root, "npm", name), packageDirectory, { recursive: true });
    if (name !== "hardgate") {
      fs.copyFileSync(nativeBinary, path.join(packageDirectory, "bin", "hardgate"));
      fs.chmodSync(path.join(packageDirectory, "bin", "hardgate"), 0o755);
    }
    run("npm", ["pack", "--json", "--loglevel=error", "--pack-destination", packagesDir], {
      cwd: packageDirectory,
      env: fixtureEnvironment(),
    });
  }
  return { packagesDir, nativeBinary };
}

try {
  const fixture = makeFixtureArchives();
  const archiveFiles = fs.readdirSync(fixture.packagesDir).filter((name) => name.endsWith(".tgz"));
  const archiveHashes = new Map(archiveFiles.map((name) => [name, hash(path.join(fixture.packagesDir, name))]));
  const previousOverride = process.env.HARDGATE_BINARY;
  process.env.HARDGATE_BINARY = path.join(fixtureRoot, "does-not-exist");
  let report;
  try {
    report = await checkPackedConsumers({
      packagesDir: fixture.packagesDir,
      binary: fixture.nativeBinary,
      version: "0.5.0",
    });
  } finally {
    if (previousOverride === undefined) delete process.env.HARDGATE_BINARY;
    else process.env.HARDGATE_BINARY = previousOverride;
  }
  assert.equal(report.hostPackage, "hardgate-linux-x64");
  assert.equal(report.consumers.map((consumer) => consumer.manager).join(","), "npm,pnpm");
  assert.equal(report.consumers.every((consumer) => consumer.nativeSha256 === report.binarySha256), true);
  assert.match(report.expectedVersionOutput, /^hardgate 0\.5\.0 \([0-9a-f]+\)$/);
  for (const [name, expected] of archiveHashes) assert.equal(hash(path.join(fixture.packagesDir, name)), expected, `${name} was rewritten`);

  const missingHost = path.join(fixtureRoot, "missing-host");
  fs.mkdirSync(missingHost);
  for (const name of archiveFiles) {
    if (!name.startsWith("hardgate-linux-x64-0.5.0")) fs.copyFileSync(path.join(fixture.packagesDir, name), path.join(missingHost, name));
  }
  await assert.rejects(
    checkPackedConsumers({ packagesDir: missingHost, binary: fixture.nativeBinary, version: "0.5.0" }),
    /missing host optional dependency hardgate-linux-x64/,
  );
  console.log("packed_consumers.test: OK");
} finally {
  fs.rmSync(fixtureRoot, { recursive: true, force: true });
}
