// Exhaust publication/resume paths with no registry access or real publication.
"use strict";
import assert from "node:assert/strict";
import { publishVerifiedPackage } from "../scripts/npm-publication-state.mjs";
import { verificationPolicy } from "../scripts/npm-verification-policy.mjs";
import { waitForNpmVersion } from "../scripts/npm-registry-state.mjs";

function scenario(options = {}) {
  const events = [];
  let present = options.existing ?? false;
  let published = false;
  let delayed = options.delay ?? 0;
  const operations = {
    probe: async () => {
      events.push("probe");
      if (options.probeError) throw options.probeError;
      if (published && delayed-- <= 0 && !options.neverVisible) present = true;
      return { state: present ? "present" : "missing" };
    },
    publish: async () => {
      events.push("publish");
      published = true;
      if (options.publishError) throw options.publishError;
    },
    verify: async () => {
      events.push("verify");
      assert.equal(present, true, "byte verification requires positive registry identity");
      if (options.mismatch) throw new Error("archive identity mismatch");
    },
  };
  return { events, operations };
}

function request() {
  const policy = verificationPolicy("0.5.0", { NPM_VERIFY_ATTEMPTS: "3", NPM_VERIFY_DELAY_SECONDS: "0", NPM_VERIFY_TIMEOUT_SECONDS: "1" });
  return { name: "hardgate-linux-x64", version: "0.5.0", policy };
}

async function successfulPaths() {
  for (const options of [{}, { existing: true }, { delay: 1 }, { publishError: { code: "ETIMEDOUT" } }, { publishError: { code: "ECONNRESET" }, delay: 1 }]) {
    const test = scenario(options);
    const result = await publishVerifiedPackage(request(), test.operations);
    assert.equal(result.state, "verified");
    assert.equal(test.events.filter((event) => event === "publish").length, options.existing ? 0 : 1);
    assert.equal(test.events.at(-1), "verify");
  }
}

async function failedPaths() {
  for (const options of [
    { existing: true, mismatch: true }, { mismatch: true },
    { neverVisible: true, publishError: { code: "ETIMEDOUT" } },
    { neverVisible: true, publishError: { code: "E403" } },
    { probeError: { code: "E401" } }, { probeError: new Error("wrong version") },
    { neverVisible: true },
  ]) {
    const test = scenario(options);
    await assert.rejects(publishVerifiedPackage(request(), test.operations));
    assert.ok(test.events.filter((event) => event === "publish").length <= 1, "failure cannot blindly republish");
    if (options.probeError) assert.equal(test.events.includes("publish"), false);
  }
}

await successfulPaths();
await failedPaths();
let probes = 0;
const observed = await waitForNpmVersion(request(), false, async () => {
  if (++probes === 1) throw { code: "E503" };
  return { state: "present" };
});
assert.equal(observed.state, "present");
assert.equal(probes, 2);
console.log("npm_publication_state.test: OK (existing, fresh, delayed, ambiguous, denied, mismatch, unavailable)");
