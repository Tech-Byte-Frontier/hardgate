// Offline fixtures for native-channel consumer contract tests.
"use strict";

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { runReleaseProcess } from "./release-process.mjs";
import { digestBytes } from "./native-channel-support.mjs";
import { verifyNativeChannel } from "./verify-native-channel.mjs";

export const version = "0.5.0";
export const sourceSha = "0123456789abcdef0123456789abcdef01234567";
const expectedLine = `hardgate ${version} (${sourceSha})`;
export const goodBinary = Buffer.from(`#!/bin/sh\nprintf '%s\\n' '${expectedLine}'\n`, "utf8");
const wrapperLauncher = Buffer.from('#!/bin/sh\nexec "$(dirname "$0")/../../../hardgate-linux-x64/bin/hardgate" "$@"\n', "utf8");

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
const binary = Buffer.from(process.env.FAKE_BINARY_B64, 'base64');
const log = process.env.FAKE_LOG;
const state = process.env.FAKE_STATE;
const attempt = state ? (Number(fs.existsSync(state) ? fs.readFileSync(state, 'utf8') : 0) + 1) : 1;
if (state) fs.writeFileSync(state, String(attempt));
if (log) fs.appendFileSync(log, JSON.stringify({spec, force: args.includes('--force'), hardgate: process.env.HARDGATE_BINARY || null, nodeOptions: process.env.NODE_OPTIONS || null, nodePath: process.env.NODE_PATH || null, tls: process.env.NODE_TLS_REJECT_UNAUTHORIZED || null, proxy: process.env.HTTPS_PROXY || null}) + '\n');
if (attempt <= Number(process.env.FAKE_NPM_FAILURES || 0)) { console.error('npm error code E503'); process.exitCode = 1; setTimeout(() => {}, 100); }
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

export function fixture(packageName = "hardgate-linux-x64") {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-native-channel-test-"));
  const npm = path.join(directory, "fake-npm.cjs");
  fs.writeFileSync(npm, fakeNpm, {mode: 0o755});
  fs.chmodSync(npm, 0o755);
  const retryNpm = path.join(directory, "retry-npm.sh");
  const shellQuote = (value) => `'${value.replaceAll("'", "'\\''")}'`;
  fs.writeFileSync(retryNpm, `#!/bin/sh
if [ ! -f "$FAKE_STATE.retry" ]; then
  : > "$FAKE_STATE.retry"
  echo 'npm error code E503' >&2
  exit 1
fi
exec ${shellQuote(process.execPath)} ${shellQuote(npm)} "$@"
`, {mode: 0o755});
  fs.chmodSync(retryNpm, 0o755);
  const archive = path.join(directory, `${packageName}.tar.gz`);
  fs.writeFileSync(archive, "archive placeholder\n");
  const wrapperSource = path.join(directory, "wrapper-source.js");
  fs.writeFileSync(wrapperSource, wrapperLauncher, {mode: 0o755});
  return {
    directory,
    npm,
    retryNpm,
    archive,
    wrapperSource,
    state: path.join(directory, "npm.state"),
    log: path.join(directory, "npm.log"),
    output: path.join(directory, "proof.json"),
  };
}

function baseEnvironment(fixtureDirectory, binary = goodBinary, overrides = {}) {
  return {
    FAKE_VERSION: version,
    FAKE_SOURCE_SHA: sourceSha,
    FAKE_BINARY_B64: binary.toString("base64"),
    FAKE_LOG: path.join(fixtureDirectory, "npm.log"),
    FAKE_STATE: path.join(fixtureDirectory, "npm.state"),
    ...overrides,
  };
}

export function verificationOptions(fixtureValue, overrides = {}) {
  return {
    host: {platform: "linux", arch: "x64", libc: "glibc"},
    runProcess: (command, args, options) => runReleaseProcess(command, args, {
      ...options,
      env: {
        ...baseEnvironment(fixtureValue.directory, overrides.binary ?? goodBinary, overrides.environment),
        ...options.env,
      },
    }),
    verifyArchive: () => ({sha256: digestBytes(goodBinary)}),
    wrapperSource: fixtureValue.wrapperSource,
    ...overrides,
    npmCommand: overrides.npmCommand === "retry" ? fixtureValue.retryNpm : (overrides.npmCommand ?? fixtureValue.npm),
  };
}

export async function successful(mode = "exact", overrides = {}) {
  const requestedPackage = overrides.packageName ?? "hardgate-linux-x64";
  const caseFixture = fixture(requestedPackage);
  try {
    const proof = await verifyNativeChannel({
      packageName: requestedPackage,
      version,
      sourceSha,
      archive: caseFixture.archive,
      mode,
      wrapperSource: requestedPackage === "hardgate-linux-x64" ? (overrides.wrapperSource ?? caseFixture.wrapperSource) : overrides.wrapperSource,
      output: caseFixture.output,
    }, verificationOptions(caseFixture, overrides));
    return {fixture: caseFixture, proof};
  } catch (error) {
    fs.rmSync(caseFixture.directory, {recursive: true, force: true});
    throw error;
  }
}
