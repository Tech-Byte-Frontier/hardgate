// Offline contract for native npm channel verification. The fake npm command
// writes real executable fixtures into isolated prefixes; no registry or Rust
// build is contacted.
"use strict";

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { runReleaseProcess } from "../scripts/release-process.mjs";
import {
  digestBytes,
  validateProof,
} from "../scripts/native-channel-support.mjs";
import {
  verifyNativeChannel,
} from "../scripts/verify-native-channel.mjs";

const version = "0.5.0";
const sourceSha = "0123456789abcdef0123456789abcdef01234567";
const expectedLine = `hardgate ${version} (${sourceSha})`;
const goodBinary = Buffer.from(`#!/bin/sh\nprintf '%s\\n' '${expectedLine}'\n`, "utf8");

const fakeNpm = String.raw`#!/usr/bin/env node
const fs = require('node:fs');
const path = require('node:path');
const args = process.argv.slice(2);
if (args[0] !== 'install') throw new Error('fake npm only supports install');
const prefix = args[args.indexOf('--prefix') + 1];
const spec = args[args.length - 1];
const marker = spec.lastIndexOf('@');
const name = spec.slice(0, marker);
const selectedVersion = process.env.FAKE_INSTALLED_VERSION || process.env.FAKE_VERSION;
const source = process.env.FAKE_SOURCE_SHA;
const binary = Buffer.from(process.env.FAKE_BINARY_B64, 'base64');
const log = process.env.FAKE_LOG;
if (log) fs.appendFileSync(log, JSON.stringify({spec, force: args.includes('--force'), hardgate: process.env.HARDGATE_BINARY || null}) + '\n');
function writeExecutable(file, bytes) {
  fs.mkdirSync(path.dirname(file), {recursive: true});
  fs.writeFileSync(file, bytes, {mode: 0o755});
  fs.chmodSync(file, 0o755);
}
function manifest(root, value) {
  fs.mkdirSync(root, {recursive: true});
  fs.writeFileSync(path.join(root, 'package.json'), JSON.stringify(value) + '\n');
}
if (name === '@tech-byte-frontier/hardgate') {
  const nodeModules = path.join(prefix, 'node_modules');
  const wrapperRoot = path.join(nodeModules, '@tech-byte-frontier', 'hardgate');
  const optional = {
    'hardgate-linux-x64': selectedVersion,
    'hardgate-linux-x64-musl': selectedVersion,
    'hardgate-linux-arm64': selectedVersion,
    'hardgate-linux-arm64-musl': selectedVersion,
    'hardgate-darwin-x64': selectedVersion,
    'hardgate-darwin-arm64': selectedVersion,
  };
  manifest(wrapperRoot, {name, version: selectedVersion, optionalDependencies: optional, bin: {hardgate: 'bin/hardgate.js'}});
  const launcher = path.join(wrapperRoot, 'bin', 'hardgate.js');
  writeExecutable(launcher, Buffer.from('#!/bin/sh\nexec "$(dirname "$0")/../../../hardgate-linux-x64/bin/hardgate" "$@"\n'));
  const nativeRoot = path.join(nodeModules, 'hardgate-linux-x64');
  manifest(nativeRoot, {name: 'hardgate-linux-x64', version: selectedVersion});
  writeExecutable(path.join(nativeRoot, 'bin', 'hardgate'), binary);
  fs.mkdirSync(path.join(nodeModules, '.bin'), {recursive: true});
  fs.symlinkSync('../@tech-byte-frontier/hardgate/bin/hardgate.js', path.join(nodeModules, '.bin', 'hardgate'));
} else {
  const root = path.join(prefix, 'node_modules', name);
  manifest(root, {name, version: selectedVersion});
  writeExecutable(path.join(root, 'bin', 'hardgate'), binary);
}
`;

function fixture() {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-native-channel-test-"));
  const npm = path.join(directory, "fake-npm.cjs");
  fs.writeFileSync(npm, fakeNpm, {mode: 0o755});
  fs.chmodSync(npm, 0o755);
  const archive = path.join(directory, "hardgate-linux-x64.tar.gz");
  fs.writeFileSync(archive, "archive placeholder\n");
  return {
    directory,
    npm,
    archive,
    log: path.join(directory, "npm.log"),
    output: path.join(directory, "proof.json"),
  };
}

function baseEnvironment(fixtureDirectory, binary = goodBinary, overrides = {}) {
  return {
    ...process.env,
    FAKE_VERSION: version,
    FAKE_SOURCE_SHA: sourceSha,
    FAKE_BINARY_B64: binary.toString("base64"),
    FAKE_LOG: path.join(fixtureDirectory, "npm.log"),
    ...overrides,
  };
}

function verificationOptions(fixture, overrides = {}) {
  return {
    host: {platform: "linux", arch: "x64", libc: "glibc"},
    npmCommand: fixture.npm,
    runProcess: (command, args, options) => runReleaseProcess(command, args, {
      ...options,
      env: baseEnvironment(fixture.directory, overrides.binary ?? goodBinary, overrides.environment),
    }),
    verifyArchive: () => ({sha256: digestBytes(goodBinary)}),
    ...overrides,
  };
}

async function successful(mode = "exact", overrides = {}) {
  const caseFixture = fixture();
  try {
    const proof = await verifyNativeChannel({
      packageName: overrides.packageName ?? "hardgate-linux-x64",
      version,
      sourceSha,
      archive: caseFixture.archive,
      mode,
      output: caseFixture.output,
    }, verificationOptions(caseFixture, overrides));
    return {fixture: caseFixture, proof};
  } catch (error) {
    fs.rmSync(caseFixture.directory, {recursive: true, force: true});
    throw error;
  }
}

const exact = await successful();
try {
  assert.deepEqual(exact.proof, {
    version,
    source_sha: sourceSha,
    mode: "exact",
    package: "hardgate-linux-x64",
    consumer: {executable: "node_modules/hardgate-linux-x64/bin/hardgate", sha256: digestBytes(goodBinary)},
    wrapper: {executable: "node_modules/hardgate-linux-x64/bin/hardgate", sha256: digestBytes(goodBinary)},
  });
  assert.deepEqual(JSON.parse(fs.readFileSync(exact.fixture.output, "utf8")), exact.proof);
  assert.equal(fs.statSync(exact.fixture.output).mode & 0o777, 0o600);
  const calls = fs.readFileSync(exact.fixture.log, "utf8").trim().split("\n").map(JSON.parse);
  assert.deepEqual(calls.map(({spec}) => spec), [
    `hardgate-linux-x64@${version}`,
    `@tech-byte-frontier/hardgate@${version}`,
  ]);
  assert.ok(calls.every(({hardgate}) => hardgate === null), "ambient HARDGATE_BINARY must not reach npm");
} finally {
  fs.rmSync(exact.fixture.directory, {recursive: true, force: true});
}

const defaultRun = await successful("default");
try {
  assert.equal(defaultRun.proof.mode, "default");
  const calls = fs.readFileSync(defaultRun.fixture.log, "utf8").trim().split("\n").map(JSON.parse);
  assert.deepEqual(calls.map(({spec}) => spec), ["hardgate-linux-x64@latest", "@tech-byte-frontier/hardgate@latest"]);
} finally {
  fs.rmSync(defaultRun.fixture.directory, {recursive: true, force: true});
}

const muslRun = await successful("exact", {
  packageName: "hardgate-linux-x64-musl",
  binary: goodBinary,
  verifyArchive: () => ({sha256: digestBytes(goodBinary)}),
});
try {
  const calls = fs.readFileSync(muslRun.fixture.log, "utf8").trim().split("\n").map(JSON.parse);
  assert.equal(calls[0].force, true, "intentional musl install on glibc must use --force");
} finally {
  fs.rmSync(muslRun.fixture.directory, {recursive: true, force: true});
}

for (const [overrides, expected] of [
  [{binary: Buffer.from("wrong bytes\n")}, /binary bytes do not match/],
  [{environment: {FAKE_INSTALLED_VERSION: "0.4.9"}}, /package identity/],
]) {
  const testFixture = fixture();
  try {
    await assert.rejects(
      verifyNativeChannel({
        packageName: "hardgate-linux-x64",
        version,
        sourceSha,
        archive: testFixture.archive,
        mode: "exact",
        output: testFixture.output,
      }, verificationOptions(testFixture, overrides)),
      expected,
    );
    assert.equal(fs.existsSync(testFixture.output), false, "failed verification must not write proof");
  } finally {
    fs.rmSync(testFixture.directory, {recursive: true, force: true});
  }
}

const unsupported = fixture();
try {
  await assert.rejects(
    verifyNativeChannel({
      packageName: "hardgate-linux-x64",
      version,
      sourceSha,
      archive: unsupported.archive,
      mode: "exact",
      output: unsupported.output,
    }, verificationOptions(unsupported, {host: {platform: "linux", arch: "arm64", libc: "glibc"}})),
    /cannot run/,
  );
  assert.equal(fs.existsSync(unsupported.log), false, "unsupported host must fail before npm");
} finally {
  fs.rmSync(unsupported.directory, {recursive: true, force: true});
}

const cleanupFailure = fixture();
let cleanupPath;
try {
  await assert.rejects(
    verifyNativeChannel({
      packageName: "hardgate-linux-x64",
      version,
      sourceSha,
      archive: cleanupFailure.archive,
      mode: "exact",
      output: cleanupFailure.output,
    }, verificationOptions(cleanupFailure, {
      cleanup: (directory) => {
        cleanupPath = directory;
        throw new Error("fixture cleanup failed");
      },
    })),
    /fixture cleanup failed/,
  );
  assert.equal(fs.existsSync(cleanupFailure.output), false, "cleanup failure must not write proof");
} finally {
  fs.rmSync(cleanupPath ?? cleanupFailure.directory, {recursive: true, force: true});
  fs.rmSync(cleanupFailure.directory, {recursive: true, force: true});
}

assert.throws(() => validateProof({
  version,
  source_sha: sourceSha,
  mode: "exact",
  package: "hardgate-linux-x64",
  consumer: {executable: "../outside", sha256: digestBytes(goodBinary)},
}), /normalized package-relative/);

console.log("native_channel_consumer.test: OK (host, exact/default specs, bytes, version, proof, cleanup)");
