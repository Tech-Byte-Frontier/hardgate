// Behavioral contract for the release-receipt checkpoint CLI.
"use strict";

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { REQUIRED_CHANNELS } from "../scripts/release-receipt-validation.mjs";
import "../scripts/release-receipt-cli.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const cli = path.join(root, "scripts", "release-receipt-cli.mjs");
const platformAssets = [
  "hardgate-linux-x64.tar.gz",
  "hardgate-linux-x64-musl.tar.gz",
  "hardgate-linux-arm64.tar.gz",
  "hardgate-linux-arm64-musl.tar.gz",
  "hardgate-darwin-x64.tar.gz",
  "hardgate-darwin-arm64.tar.gz",
];
const version = "0.5.0";

const h40 = (letter) => letter.repeat(40);
const digest = (file) => crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");

function runCli(args) {
  const result = spawnSync(process.execPath, [cli, ...args], { cwd: root, encoding: "utf8" });
  assert.equal(result.error, undefined, result.error?.message);
  return result;
}

function makeDist(directory) {
  fs.mkdirSync(directory, { recursive: true });
  const names = [...platformAssets, "SHA256SUMS", `hardgate-${version}.sbom.cdx.json`];
  for (const name of names) fs.writeFileSync(path.join(directory, name), `fixture:${name}\n`);
  return names.sort();
}

function createArgs(dist, receipt, overrides = {}) {
  const values = {
    "--output": receipt,
    "--version": version,
    "--source-sha": h40("a"),
    "--tooling-sha": h40("b"),
    "--tag-object": h40("c"),
    "--build-run-id": "33926961536",
    "--artifact-id": "987654321",
    "--dist": dist,
    ...overrides,
  };
  return ["create", ...Object.entries(values).flatMap(([key, value]) => [key, value])];
}

function makeReceipt(directory, suffix = "receipt.json") {
  const dist = path.join(directory, "dist");
  const receipt = path.join(directory, suffix);
  makeDist(dist);
  const result = runCli(createArgs(dist, receipt));
  assert.equal(result.status, 0, result.stderr);
  return { dist, receipt };
}

const directory = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-release-receipt-cli-"));
try {
  const created = makeReceipt(directory);
  const value = JSON.parse(fs.readFileSync(created.receipt, "utf8"));
  const expectedNames = [...platformAssets, "SHA256SUMS", `hardgate-${version}.sbom.cdx.json`].sort();
  assert.deepEqual(value.identity.archives.map(({ name }) => name), expectedNames);
  for (const archive of value.identity.archives) {
    assert.equal(archive.sha256, digest(path.join(created.dist, archive.name)));
  }

  const channel = REQUIRED_CHANNELS[0];
  let result = runCli(["advance", "--receipt", created.receipt, "--channel", channel, "--to", "staged"]);
  assert.equal(result.status, 0, result.stderr);
  const advancedBytes = fs.readFileSync(created.receipt);
  result = runCli(createArgs(created.dist, created.receipt));
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(fs.readFileSync(created.receipt), advancedBytes, "same identity must preserve an advanced receipt");
  result = runCli(createArgs(created.dist, created.receipt, { "--tooling-sha": h40("d") }));
  assert.notEqual(result.status, 0);
  assert.deepEqual(fs.readFileSync(created.receipt), advancedBytes, "mismatched identity must not overwrite a receipt");

  const missing = path.join(directory, "missing-dist");
  makeDist(missing);
  fs.unlinkSync(path.join(missing, "SHA256SUMS"));
  result = runCli(createArgs(missing, path.join(directory, "missing.json")));
  assert.notEqual(result.status, 0);
  const extra = path.join(directory, "extra-dist");
  makeDist(extra);
  fs.writeFileSync(path.join(extra, "unexpected.txt"), "extra\n");
  result = runCli(createArgs(extra, path.join(directory, "extra.json")));
  assert.notEqual(result.status, 0);
  const linked = path.join(directory, "linked-dist");
  makeDist(linked);
  fs.unlinkSync(path.join(linked, platformAssets[1]));
  fs.symlinkSync(path.join(linked, platformAssets[0]), path.join(linked, platformAssets[1]));
  result = runCli(createArgs(linked, path.join(directory, "linked.json")));
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /symbolic link/);

  const binary = path.join(directory, "hardgate");
  const binaryLink = path.join(directory, "bin-link");
  fs.writeFileSync(binary, "hardgate consumer v1\n");
  fs.symlinkSync(binary, binaryLink);
  const consumerChannel = REQUIRED_CHANNELS[1];
  const consumerReceipt = makeReceipt(directory, "consumer.json").receipt;
  const advance = (target, consumer = undefined) => {
    const args = ["advance", "--receipt", consumerReceipt, "--channel", consumerChannel, "--to", target];
    if (consumer) args.push("--consumer", consumer);
    return runCli(args);
  };
  assert.equal(advance("immutable_verified").status, 1);
  assert.equal(advance("staged").status, 0);
  assert.equal(advance("immutable_verified").status, 0);
  result = advance("exact_consumer_verified");
  assert.notEqual(result.status, 0);
  result = advance("exact_consumer_verified", binaryLink);
  assert.equal(result.status, 0, result.stderr);
  result = advance("promoted", binaryLink);
  assert.notEqual(result.status, 0);
  assert.equal(advance("promoted").status, 0);
  assert.equal(advance("default_consumer_verified", binaryLink).status, 0);
  const consumerValue = JSON.parse(fs.readFileSync(consumerReceipt, "utf8"));
  const exactEvent = consumerValue.channels[consumerChannel].events[2];
  assert.equal(exactEvent.evidence.consumer.executable, path.normalize(fs.realpathSync(binary)));
  assert.equal(exactEvent.evidence.consumer.sha256, digest(binary));
  const replayBytes = fs.readFileSync(consumerReceipt);
  assert.equal(advance("default_consumer_verified", binaryLink).status, 0);
  assert.deepEqual(fs.readFileSync(consumerReceipt), replayBytes, "matching replay must be byte-preserving");
  fs.writeFileSync(binary, "hardgate consumer v2\n");
  result = advance("default_consumer_verified", binaryLink);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /replayed transition evidence/);
  assert.deepEqual(fs.readFileSync(consumerReceipt), replayBytes, "changed consumer bytes must not mutate the receipt");
  assert.equal(runCli(["advance", "--receipt", consumerReceipt, "--channel", consumerChannel, "--to", "staged", "--consumer", binary]).status, 1);

  const failedReceipt = makeReceipt(directory, "failed.json").receipt;
  result = runCli(["failure", "--receipt", failedReceipt, "--channel", REQUIRED_CHANNELS[2], "--code", "registry-timeout", "--message", "probe deadline elapsed"]);
  assert.equal(result.status, 0, result.stderr);
  result = runCli(["failure", "--receipt", failedReceipt, "--channel", REQUIRED_CHANNELS[2], "--code", "retry", "--message", "retry requested"]);
  assert.equal(result.status, 0, result.stderr);
  const failedValue = JSON.parse(fs.readFileSync(failedReceipt, "utf8"));
  assert.equal(failedValue.channels[REQUIRED_CHANNELS[2]].events.length, 2);
  assert.match(runCli(["assert", "--receipt", failedReceipt]).stdout, /pending=9/);
  assert.equal(runCli(["assert", "--receipt", failedReceipt, "--require-complete"]).status, 1);
  result = runCli(["failure", "--receipt", failedReceipt, "--channel", REQUIRED_CHANNELS[2], "--code", "BAD!", "--message", "invalid"]);
  assert.notEqual(result.status, 0);
  result = runCli(["failure", "--receipt", failedReceipt, "--channel", REQUIRED_CHANNELS[2], "--code", "secret", "--message", "Authorization: super-secret-value"]);
  assert.notEqual(result.status, 0);
  assert.doesNotMatch(result.stderr, /super-secret-value/);

  assert.notEqual(runCli(["assert", "--receipt", failedReceipt, "--receipt", failedReceipt]).status, 0);
  assert.notEqual(runCli(["assert", "--receipt", failedReceipt, "--unknown"]).status, 0);
  assert.notEqual(runCli(["assert"]).status, 0);
} finally {
  fs.rmSync(directory, { recursive: true, force: true });
}

const malformedDirectory = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-release-receipt-cli-malformed-"));
try {
  const malformed = path.join(malformedDirectory, "malformed.json");
  fs.writeFileSync(malformed, "{}\n");
  assert.notEqual(runCli(["assert", "--receipt", malformed]).status, 0);
  const target = path.join(malformedDirectory, "target.json");
  fs.writeFileSync(target, "{}\n");
  const link = path.join(malformedDirectory, "receipt-link.json");
  fs.symlinkSync(target, link);
  const result = runCli(["assert", "--receipt", link]);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /symbolic-link/);
} finally {
  fs.rmSync(malformedDirectory, { recursive: true, force: true });
}

console.log("release_receipt_cli: create, checkpoint, consumer, failure, and strict-read contracts verified");
