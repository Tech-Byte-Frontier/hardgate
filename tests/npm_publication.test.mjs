// Exercise the actual verifier with local subprocess/registry fault injection.
"use strict";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { isRetryableNpmPackError, retryAfterMs } from "../scripts/npm-pack-retry.mjs";
import { verificationPolicy } from "../scripts/npm-verification-policy.mjs";
import { projectRoot } from "../scripts/release-support.mjs";
import { fixtureEnvironment, publicationFixture, stagePackage } from "./npm_publication.fixture.mjs";

const fixture = publicationFixture();

function verify(environment = {}, version = "0.5.0") {
  return spawnSync(process.execPath, [path.join(projectRoot, "scripts/verify-npm-publication.mjs"), "--version", version, "--dist", path.join(fixture.directory, "dist"), "--platform-only", "--package", "hardgate-linux-x64"], {
    encoding: "utf8", env: fixtureEnvironment(fixture, environment), timeout: 15000,
  });
}

function assertCase(mode, expectedAttempts, environment = {}) {
  stagePackage(fixture);
  const result = verify({ FIXTURE_MODE: mode, ...environment });
  assert.equal(Number(fs.readFileSync(fixture.state)), expectedAttempts, result.stderr);
  return result;
}

function checkInputValidation() {
  for (const [name, value] of [["NPM_VERIFY_ATTEMPTS", "3oops"], ["NPM_VERIFY_DELAY_SECONDS", "-1"], ["NPM_VERIFY_TIMEOUT_SECONDS", "Infinity"], ["NPM_VERIFY_CHILD_TIMEOUT_SECONDS", "0"]]) {
    assert.throws(() => verificationPolicy("0.5.0", { [name]: value }), new RegExp(name));
  }
  for (const version of ["latest", "0.5", "01.5.0", "0.5.0-01", "0.5.0;true", "--dist"]) {
    stagePackage(fixture);
    assert.notEqual(verify({}, version).status, 0);
    assert.equal(fs.existsSync(fixture.state), false, "bad version must fail before subprocess launch");
  }
}

function checkRegistryFailures() {
  for (const mode of ["ETARGET", "EAI_AGAIN", "ECONNRESET", "E429", "E503"]) assert.equal(assertCase(mode, 2).status, 0, mode);
  for (const mode of ["E401", "E403", "EINTEGRITY", "EUSAGE", "unclassified"]) assert.notEqual(assertCase(mode, 1).status, 0, mode);
  for (const metadata of ["missing", "auth", "wrong", "malformed"]) assert.notEqual(assertCase("ETARGET", 1, { FIXTURE_METADATA: metadata }).status, 0, metadata);
  for (const mode of ["ETARGET-forever", "E404-forever"]) assert.notEqual(assertCase(mode, 3).status, 0, mode);
}

function checkArchiveIntegrity() {
  for (const [overrides, message] of [
    [{ binary: "wrong bytes" }, /does not byte-match/],
    [{ manifest: { version: "0.4.2" } }, /manifest identity/],
    [{ manifest: { cpu: ["arm64"] } }, /manifest cpu/],
    [{ mode: 0o644 }, /executable mode/],
  ]) {
    stagePackage(fixture, overrides);
    const result = verify();
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, message);
    assert.equal(Number(fs.readFileSync(fixture.state)), 1, "integrity failures cannot be retried");
  }
}

function checkStalledChild() {
  stagePackage(fixture);
  const started = performance.now();
  const result = verify({ FIXTURE_MODE: "stall", NPM_VERIFY_TIMEOUT_SECONDS: "2", NPM_VERIFY_CHILD_TIMEOUT_SECONDS: "1" });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /deadline/);
  assert.ok(performance.now() - started < 4000, "operation deadline must bound a stalled npm process");
  const pid = Number(fs.readFileSync(`${fixture.state}.pid`));
  assert.throws(() => process.kill(pid, 0), { code: "ESRCH" }, "stalled child must be reaped");
}

try {
  assert.equal(isRetryableNpmPackError("ETARGET"), false);
  assert.equal(isRetryableNpmPackError("ETARGET", { exactVersionObserved: true }), true);
  assert.equal(isRetryableNpmPackError("E403 404 ETARGET", { exactVersionObserved: true }), false);
  assert.equal(retryAfterMs("Retry-After: 7"), 7000);
  checkInputValidation();
  checkRegistryFailures();
  checkArchiveIntegrity();
  checkStalledChild();
} finally {
  fs.rmSync(fixture.directory, { recursive: true, force: true });
}
console.log("npm_publication.test: OK (context, deadlines, retries, identity, no publication)");
