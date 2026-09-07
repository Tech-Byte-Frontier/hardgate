"use strict";

import "./native_archive_identity.test.mjs";

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import {
  fixture,
  goodBinary,
  sourceSha,
  successful,
  verificationOptions,
  version,
} from "../scripts/native-channel-test-fixtures.mjs";
import {
  digestBytes,
  digestFile,
  detectHost,
  parseArgs,
  restrictedPath,
  sanitizedEnvironment,
} from "../scripts/native-channel-support.mjs";
import { validateProof } from "../scripts/native-channel-proof.mjs";
import {
  verifyNativeArchive,
  verifyNativeChannel,
} from "../scripts/verify-native-channel.mjs";

function nativeChannelRoots() {
  return fs.readdirSync(os.tmpdir())
    .filter((entry) => entry.startsWith("hardgate-native-channel-") && !entry.startsWith("hardgate-native-channel-test-"))
    .sort();
}

async function withEnvironment(values, action) {
  const previous = Object.fromEntries(Object.keys(values).map((key) => [key, process.env[key]]));
  Object.assign(process.env, values);
  try {
    return await action();
  } finally {
    for (const [key, value] of Object.entries(previous)) if (value === undefined) delete process.env[key]; else process.env[key] = value;
  }
}

function assertCleanChildCall(call) {
  for (const key of ["hardgate", "nodeOptions", "nodePath", "tls", "proxy", "ldPreload", "ldLibraryPath", "extraCaCerts", "sslCertFile", "sslCertDir", "gitConfigGlobal", "gitConfigSystem"]) {
    assert.equal(call[key], null, `${key} must not reach npm`);
  }
  assert.notEqual(call.home, injectedEnvironment.HOME);
  assert.equal(call.npmConfigRegistry, "https://registry.npmjs.org/");
  assert.notEqual(call.npmConfigUser, injectedEnvironment.NPM_CONFIG_USERCONFIG);
  assert.notEqual(call.npmConfigCache, injectedEnvironment.NPM_CONFIG_CACHE);
  assert.equal(call.path, restrictedPath());
  assert.notEqual(call.tempDir, injectedEnvironment.TMPDIR);
}

const injectedEnvironment = {
  HOME: "/tmp/ambient-home", TMPDIR: "/tmp", LD_PRELOAD: "/tmp/ambient-preload.so", LD_LIBRARY_PATH: "/tmp/ambient-libraries",
  NODE_EXTRA_CA_CERTS: "/tmp/ambient-ca.pem", SSL_CERT_FILE: "/tmp/ambient-cert.pem", SSL_CERT_DIR: "/tmp/ambient-certs",
  GIT_CONFIG_GLOBAL: "/tmp/ambient-gitconfig", GIT_CONFIG_SYSTEM: "/tmp/ambient-gitsystem", NPM_CONFIG_REGISTRY: "https://ambient.example.invalid/",
  NPM_CONFIG_USERCONFIG: "/tmp/ambient-user.npmrc", NPM_CONFIG_CACHE: "/tmp/ambient-cache", NODE_OPTIONS: "--require=/tmp/ambient.js",
};
const sanitized = sanitizedEnvironment(injectedEnvironment);
assert.equal(sanitized.PATH, restrictedPath());
for (const key of Object.keys(injectedEnvironment)) assert.equal(sanitized[key], undefined, `${key} must not reach child tools`);

const exact = await withEnvironment(injectedEnvironment, () => successful());
try {
  assert.deepEqual(exact.proof, {
    schema_version: 1,
    version,
    source_sha: sourceSha,
    mode: "exact",
    package: "hardgate-linux-x64",
    archive: {name: "hardgate-linux-x64.tar.gz", sha256: digestFile(exact.fixture.archive)},
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
  for (const call of calls) assertCleanChildCall(call);
} finally {
  fs.rmSync(exact.fixture.directory, {recursive: true, force: true});
}

for (const packageName of ["hardgate-linux-x64-musl", "hardgate-win32-x64", "hardgate-win32-arm64", "__proto__"]) {
  assert.throws(() => parseArgs(["--package", packageName, "--version", version, "--source-sha", sourceSha, "--archive", "/tmp/archive.tar.gz", "--mode", "exact", "--output", "/tmp/native-proof.json"]), /supported native package/);
}

const defaultRun = await successful("default");
try {
  assert.equal(defaultRun.proof.mode, "default");
  const calls = fs.readFileSync(defaultRun.fixture.log, "utf8").trim().split("\n").map(JSON.parse);
  assert.deepEqual(calls.map(({spec}) => spec), ["hardgate-linux-x64@latest", "@tech-byte-frontier/hardgate@latest"]);
} finally {
  fs.rmSync(defaultRun.fixture.directory, {recursive: true, force: true});
}

const retryRun = await successful("exact", {
  npmCommand: "retry",
  policy: {attempts: 3, delayMs: 0, childMs: 2_000, deadline: performance.now() + 10_000},
});
try {
  const calls = fs.readFileSync(retryRun.fixture.log, "utf8").trim().split("\n").map(JSON.parse);
  assert.equal(calls.length, 2, "one transient npm error should be retried before wrapper verification");
  assert.equal(fs.existsSync(`${retryRun.fixture.state}.retry`), true, "the transient npm failure must precede a successful retry");
} finally {
  fs.rmSync(retryRun.fixture.directory, {recursive: true, force: true});
}

const deadlineFixture = fixture();
try {
  await assert.rejects(
    verifyNativeChannel({
      packageName: "hardgate-linux-x64",
      version,
      sourceSha,
      archive: deadlineFixture.archive,
      mode: "exact",
      wrapperSource: deadlineFixture.wrapperSource,
    }, verificationOptions(deadlineFixture, {
      npmCommand: "retry",
      policy: {attempts: 3, delayMs: 100, childMs: 2_000, deadline: performance.now() + 20},
    })),
    /deadline/,
    "retry backoff must honor the operation deadline",
  );
} finally {
  fs.rmSync(deadlineFixture.directory, {recursive: true, force: true});
}

const missingWrapper = fixture();
try {
  await assert.rejects(
    verifyNativeChannel({
      packageName: "hardgate-linux-x64",
      version,
      sourceSha,
      archive: missingWrapper.archive,
      mode: "exact",
      output: missingWrapper.output,
    }, verificationOptions(missingWrapper)),
    /wrapper-source is required/,
  );
  assert.equal(fs.existsSync(missingWrapper.log), false, "canonical wrapper source is checked before npm");
} finally {
  fs.rmSync(missingWrapper.directory, {recursive: true, force: true});
}

for (const [overrides, expected] of [
  [{binary: Buffer.from("wrong bytes\n")}, /binary bytes do not match/],
  [{environment: {FAKE_INSTALLED_VERSION: "0.4.9"}}, /package identity/],
]) {
  const rootsBeforeFailure = nativeChannelRoots();
  const testFixture = fixture();
  try {
    await assert.rejects(
      verifyNativeChannel({
        packageName: "hardgate-linux-x64",
        version,
        sourceSha,
        archive: testFixture.archive,
        mode: "exact",
        wrapperSource: testFixture.wrapperSource,
        output: testFixture.output,
      }, verificationOptions(testFixture, overrides)),
      expected,
    );
    assert.equal(fs.existsSync(testFixture.output), false, "failed verification must not write proof");
    assert.deepEqual(nativeChannelRoots(), rootsBeforeFailure, "failed verification must clean its temporary root");
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

assert.equal(detectHost({platform: "linux", arch: "x64", glibcVersion: null, sharedObjects: [], lddOutput: ""}).libc, null, "unknown Linux libc must remain unknown");
const unknownLibc = fixture();
try {
  await assert.rejects(
    verifyNativeChannel({
      packageName: "hardgate-linux-x64",
      version,
      sourceSha,
      archive: unknownLibc.archive,
      mode: "exact",
      output: unknownLibc.output,
      wrapperSource: unknownLibc.wrapperSource,
    }, verificationOptions(unknownLibc, {host: {platform: "linux", arch: "x64", libc: null}})),
    /libc could not be identified/,
  );
} finally {
  fs.rmSync(unknownLibc.directory, {recursive: true, force: true});
}

const symlinkArchive = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-native-channel-archive-test-"));
try {
  const archiveRoot = path.join(symlinkArchive, "hardgate-linux-x64");
  fs.mkdirSync(archiveRoot, {recursive: true});
  fs.writeFileSync(path.join(archiveRoot, "hardgate"), goodBinary, {mode: 0o755});
  fs.chmodSync(path.join(archiveRoot, "hardgate"), 0o755);
  fs.symlinkSync("hardgate", path.join(archiveRoot, "BUILD-METADATA.json"));
  const archive = path.join(symlinkArchive, "hardgate-linux-x64.tar.gz");
  const tar = spawnSync("/usr/bin/tar", ["-czf", archive, "-C", symlinkArchive, "hardgate-linux-x64"], {encoding: "utf8"});
  assert.equal(tar.status, 0, tar.stderr);
  await assert.rejects(
    verifyNativeArchive({
      archive,
      packageName: "hardgate-linux-x64",
      version,
      sourceSha,
      directory: path.join(symlinkArchive, "extract"),
    }),
    /bounded regular file/,
    "archive metadata symlinks must be rejected before extraction",
  );
} finally {
  fs.rmSync(symlinkArchive, {recursive: true, force: true});
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
      wrapperSource: cleanupFailure.wrapperSource,
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
  schema_version: 1,
  version,
  source_sha: sourceSha,
  mode: "exact",
  package: "hardgate-linux-x64",
  archive: {name: "hardgate-linux-x64.tar.gz", sha256: digestBytes(goodBinary)},
  consumer: {executable: "../outside", sha256: digestBytes(goodBinary)},
}), /normalized package-relative/);

const strictProof = {
  schema_version: 1,
  version,
  source_sha: sourceSha,
  mode: "exact",
  package: "hardgate-linux-x64",
  archive: {name: "hardgate-linux-x64.tar.gz", sha256: digestBytes(goodBinary)},
  consumer: {executable: "node_modules/hardgate-linux-x64/bin/hardgate", sha256: digestBytes(goodBinary)},
};
assert.throws(() => validateProof(strictProof), /wrapper is required/);
assert.throws(() => validateProof({...strictProof, schema_version: 2, wrapper: strictProof.consumer}), /schema_version is unsupported/);
assert.throws(() => validateProof({...strictProof, source_sha: `${sourceSha}abcdef`, wrapper: strictProof.consumer}), /40-character/);
assert.throws(() => validateProof({...strictProof, archive: {...strictProof.archive, extra: true}, wrapper: strictProof.consumer}), /archive has unexpected fields/);

assert.throws(() => validateProof({
  schema_version: 1,
  version,
  source_sha: sourceSha,
  mode: "exact",
  package: "hardgate-linux-x64-musl",
  archive: {name: "hardgate-linux-x64-musl.tar.gz", sha256: digestBytes(goodBinary)},
  consumer: {executable: "node_modules/hardgate-linux-x64-musl/bin/hardgate", sha256: digestBytes(goodBinary)},
  wrapper: {executable: "node_modules/hardgate-linux-x64-musl/bin/hardgate", sha256: digestBytes(goodBinary)},
}), /supported native package/);

console.log("native_channel_consumer.test: OK (host, exact/default specs, bytes, version, proof, cleanup)");
