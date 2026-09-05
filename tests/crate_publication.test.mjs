// Exercise crate publication proofing with a real tar archive and fake public endpoints.
"use strict";

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { performance } from "node:perf_hooks";
import { runReleaseProcess } from "../scripts/release-process.mjs";
import { verificationPolicy } from "../scripts/npm-verification-policy.mjs";
import { verifyCratePublication } from "../scripts/verify-crate-publication.mjs";

const sourceSha = "0123456789abcdef0123456789abcdef01234567";
const version = "0.5.0";

function digest(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex");
}

function policy(overrides = {}) {
  return verificationPolicy(version, {
    NPM_VERIFY_ATTEMPTS: "3",
    NPM_VERIFY_DELAY_SECONDS: "0",
    NPM_VERIFY_TIMEOUT_SECONDS: "10",
    NPM_VERIFY_CHILD_TIMEOUT_SECONDS: "2",
    ...overrides,
  });
}

function runTar(args) {
  const result = spawnSync("/usr/bin/tar", args, { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
}

function fixture({ dirty = false, includeDirty = true } = {}) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-crate-publication-test-"));
  const source = path.join(directory, "hardgate-" + version);
  fs.mkdirSync(source, { recursive: true });
  const git = { sha1: sourceSha };
  if (includeDirty) git.dirty = dirty;
  fs.writeFileSync(path.join(source, ".cargo_vcs_info.json"), JSON.stringify({ git, path_in_vcs: "" }));
  fs.writeFileSync(path.join(source, "Cargo.toml"), "[package]\nname = \"hardgate\"\n");
  const expected = path.join(directory, "hardgate.crate");
  runTar(["-czf", expected, "-C", directory, "hardgate-" + version]);
  const bytes = fs.readFileSync(expected);
  return { directory, expected, bytes, sha256: digest(bytes), source };
}

function fakeRunner(fixtureData, options = {}) {
  const calls = [];
  let apiCalls = 0;
  let downloadCalls = 0;
  const run = async (command, args, childOptions) => {
    calls.push({ command, args: [...args], env: { ...childOptions.env }, timeoutMs: childOptions.timeoutMs });
    if (command === "/usr/bin/tar") return runReleaseProcess(command, args, childOptions);
    assert.equal(command, "/fake/curl");
    if (args.includes("--output")) {
      downloadCalls += 1;
      const destination = args[args.indexOf("--output") + 1];
      const status = options.downloadStatuses?.[downloadCalls - 1] ?? 200;
      if (status === 200) fs.writeFileSync(destination, options.downloadBytes ?? fixtureData.bytes);
      return "\n" + status;
    }
    apiCalls += 1;
    const status = options.apiStatuses?.[apiCalls - 1] ?? 200;
    if (status !== 200) return "{}\n" + status;
    if (args.at(-1) === "https://crates.io/api/v1/crates/hardgate") {
      return JSON.stringify({ crate: { max_stable_version: options.defaultVersion ?? version } }) + "\n200";
    }
    const metadata = {
      version: {
        num: options.metadataVersion ?? version,
        yanked: options.yanked ?? false,
        checksum: options.checksum ?? fixtureData.sha256,
      },
    };
    return JSON.stringify(metadata) + "\n200";
  };
  return { calls, run, get apiCalls() { return apiCalls; }, get downloadCalls() { return downloadCalls; } };
}

async function verify(data, fake, options = {}) {
  return verifyCratePublication({ expected: data.expected, version, sourceSha, ...options.request }, {
    policy: policy(),
    run: fake.run,
    curlCommand: "/fake/curl",
    tempDirectory: data.directory,
    ...options.operations,
  });
}

async function successfulProof() {
  const data = fixture();
  const secretNames = ["CARGO_REGISTRY_TOKEN", "GITHUB_TOKEN", "NPM_TOKEN", "TAR_OPTIONS"];
  const savedSecrets = Object.fromEntries(secretNames.map((name) => [name, process.env[name]]));
  try {
    for (const name of secretNames) process.env[name] = "fixture-secret";
    const fake = fakeRunner(data);
    const result = await verify(data, fake);
    assert.deepEqual(result, { version, source_sha: sourceSha, sha256: data.sha256 });
    assert.equal(fake.apiCalls, 1);
    assert.equal(fake.downloadCalls, 1);
    assert.equal(fs.readdirSync(data.directory).some((name) => name.startsWith("hardgate-crate-publication-")), false, "temporary archive directory must be cleaned");
    const curlCalls = fake.calls.filter((call) => call.command === "/fake/curl");
    assert.equal(curlCalls.length, 2);
    for (const call of curlCalls) {
      assert.deepEqual(call.args.slice(call.args.indexOf("--user-agent"), call.args.indexOf("--user-agent") + 2), [
        "--user-agent", "hardgate-release (https://github.com/Tech-Byte-Frontier/hardgate)",
      ]);
      assert.equal(call.args.includes("--max-redirs"), true);
    }
    assert.equal(curlCalls[0].args.at(-1), "https://crates.io/api/v1/crates/hardgate/0.5.0");
    assert.equal(curlCalls[1].args.at(-1), "https://static.crates.io/crates/hardgate/hardgate-0.5.0.crate");
    const childEnvironments = fake.calls.map((call) => call.env);
    assert.ok(childEnvironments.every((environment) => environment.CARGO_REGISTRY_TOKEN === undefined));
    assert.ok(childEnvironments.every((environment) => environment.GITHUB_TOKEN === undefined));
    assert.ok(childEnvironments.every((environment) => environment.NPM_TOKEN === undefined));
    assert.ok(childEnvironments.every((environment) => environment.TAR_OPTIONS === undefined));
  } finally {
    for (const name of secretNames) {
      if (savedSecrets[name] === undefined) delete process.env[name];
      else process.env[name] = savedSecrets[name];
    }
    fs.rmSync(data.directory, { recursive: true, force: true });
  }
}

async function defaultProof() {
  for (const options of [{}, { defaultVersion: "0.4.2" }, { defaultVersion: undefined }]) {
    const data = fixture();
    try {
      const fake = fakeRunner(data, options);
      if (options.defaultVersion) {
        await assert.rejects(verify(data, fake, { request: { requireDefault: true } }), /default stable/);
        assert.equal(fake.apiCalls, 2);
      } else {
        await verify(data, fake, { request: { requireDefault: true } });
        assert.equal(fake.apiCalls, 2);
      }
      const apiCalls = fake.calls.filter((call) => call.command === "/fake/curl");
      assert.equal(apiCalls[0].args.at(-1), "https://crates.io/api/v1/crates/hardgate/0.5.0");
      assert.equal(apiCalls[1].args.at(-1), "https://crates.io/api/v1/crates/hardgate");
    } finally {
      fs.rmSync(data.directory, { recursive: true, force: true });
    }
  }
}

async function dirtyIdentityCases() {
  const cleanWithoutDirty = fixture({ includeDirty: false });
  try {
    await verify(cleanWithoutDirty, fakeRunner(cleanWithoutDirty));
  } finally {
    fs.rmSync(cleanWithoutDirty.directory, { recursive: true, force: true });
  }
  for (const dirty of [true, "false", null, 0]) {
    const data = fixture({ dirty });
    try {
      await assert.rejects(verify(data, fakeRunner(data)), /dirty or malformed Cargo VCS identity/);
    } finally {
      fs.rmSync(data.directory, { recursive: true, force: true });
    }
  }
}

async function retryAndFailureCases() {
  for (const [options, expectedAttempts] of [
    [{ apiStatuses: [404, 200] }, 2],
    [{ apiStatuses: [429, 503, 200] }, 3],
  ]) {
    const data = fixture();
    try {
      const fake = fakeRunner(data, options);
      await verify(data, fake);
      assert.equal(fake.apiCalls, expectedAttempts);
    } finally {
      fs.rmSync(data.directory, { recursive: true, force: true });
    }
  }
  for (const options of [
    { checksum: "f".repeat(64), message: /checksum/ },
    { yanked: true, message: /yanked/ },
    { metadataVersion: "0.4.2", message: /version/ },
  ]) {
    const data = fixture();
    try {
      const fake = fakeRunner(data, options);
      await assert.rejects(verify(data, fake), options.message);
      assert.equal(fake.apiCalls, 1, "identity failures cannot be retried");
      assert.equal(fake.downloadCalls, 0);
      assert.equal(fs.readdirSync(data.directory).some((name) => name.startsWith("hardgate-crate-publication-")), false);
    } finally {
      fs.rmSync(data.directory, { recursive: true, force: true });
    }
  }
  {
    const data = fixture();
    try {
      const fake = fakeRunner(data, { downloadBytes: Buffer.from("different archive bytes") });
      await assert.rejects(verify(data, fake), /byte-match/);
      assert.equal(fake.downloadCalls, 1, "archive byte mismatches cannot be retried");
    } finally {
      fs.rmSync(data.directory, { recursive: true, force: true });
    }
  }
}

async function archiveAndInputFailures() {
  const data = fixture();
  try {
    await assert.rejects(verify(data, fakeRunner(data), { request: { sourceSha: sourceSha.toUpperCase() } }), /lowercase/);
    await assert.rejects(verify(data, fakeRunner(data), { operations: { tarCommand: "/missing/tar" } }), /identity/i);
    const link = path.join(data.directory, "link.crate");
    fs.symlinkSync(data.expected, link);
    await assert.rejects(verify(data, fakeRunner(data), { request: { expected: link } }), /regular file/);
    const duplicate = path.join(data.directory, "duplicate.crate");
    runTar(["-czf", duplicate, "-C", data.directory, "hardgate-" + version + "/.cargo_vcs_info.json", "hardgate-" + version + "/.cargo_vcs_info.json"]);
    await assert.rejects(verify(data, fakeRunner(data), { request: { expected: duplicate } }), /malformed|duplicate/);
  } finally {
    fs.rmSync(data.directory, { recursive: true, force: true });
  }
}

async function boundedChildAndCleanup() {
  const data = fixture();
  const stall = path.join(data.directory, "stall-curl.mjs");
  fs.writeFileSync(stall, "setInterval(() => {}, 1000);\n");
  try {
    const started = performance.now();
    const fake = fakeRunner(data);
    await assert.rejects(verify(data, fake, {
      operations: {
        policy: policy({ NPM_VERIFY_ATTEMPTS: "1", NPM_VERIFY_TIMEOUT_SECONDS: "2", NPM_VERIFY_CHILD_TIMEOUT_SECONDS: "1" }),
        curlCommand: process.execPath,
        run: (command, args, options) => command === "/usr/bin/tar"
          ? runReleaseProcess(command, args, options)
          : runReleaseProcess(process.execPath, [stall], options),
      },
    }), /public version/);
    assert.ok(performance.now() - started < 4000, "stalled curl must be bounded by child and operation deadlines");
    assert.equal(fs.readdirSync(data.directory).some((name) => name.startsWith("hardgate-crate-publication-")), false, "temporary archive directory must be cleaned after timeout");
  } finally {
    fs.rmSync(data.directory, { recursive: true, force: true });
  }
}

await successfulProof();
await defaultProof();
await dirtyIdentityCases();
await retryAndFailureCases();
await archiveAndInputFailures();
await boundedChildAndCleanup();
console.log("crate_publication.test: OK (archive identity, metadata, retries, bounded public proof, cleanup)");
