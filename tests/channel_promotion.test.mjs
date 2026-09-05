// Behavioral contract for one-shot, independently verified channel promotion.
"use strict";

import assert from "node:assert/strict";
import { performance } from "node:perf_hooks";
import { promoteVerifiedChannel } from "../scripts/channel-promotion.mjs";

const target = "1.2.0";

function policy(overrides = {}) {
  return { attempts: 3, delayMs: 0, deadline: performance.now() + 2_000, ...overrides };
}

function request(overrides = {}) {
  return { version: target, exactConsumerVerified: true, policy: policy(), ...overrides };
}

function operations(sequence, overrides = {}) {
  const events = [];
  let probes = [...sequence];
  const nextProbe = async (input) => {
    events.push("probe");
    const value = probes.shift();
    if (value instanceof Error) throw value;
    return value ?? { state: "present", version: target };
  };
  return {
    events,
    operations: {
      probe: overrides.probe ?? nextProbe,
      promote: async (input) => { events.push("promote"); return overrides.promote?.(input); },
      verifyImmutable: async (input) => { events.push("immutable"); return overrides.verifyImmutable?.(input); },
      verifyDefault: async (input) => { events.push("default"); return overrides.verifyDefault?.(input); },
    },
  };
}

async function rejects(action, pattern) {
  await assert.rejects(action, pattern);
}

const existing = operations([{ state: "present", version: target }]);
assert.deepEqual(await promoteVerifiedChannel(request(), existing.operations), { publication: "existing", state: "default_consumer_verified" });
assert.deepEqual(existing.events, ["probe", "default"]);

const promoted = operations([{ state: "present", version: "1.1.0" }, { state: "present", version: target }]);
assert.deepEqual(await promoteVerifiedChannel(request(), promoted.operations), { publication: "promoted", state: "default_consumer_verified" });
assert.deepEqual(promoted.events, ["probe", "immutable", "promote", "probe", "default"]);

const missing = operations([{ state: "missing", version: target }, { state: "present", version: target }]);
assert.equal((await promoteVerifiedChannel(request(), missing.operations)).publication, "promoted");
assert.deepEqual(missing.events, ["probe", "immutable", "promote", "probe", "default"]);

const ambiguous = operations([new Error("ETIMEDOUT"), { state: "present", version: "1.1.0" }, { state: "present", version: "1.1.0" }, { state: "present", version: target }], {
  promote: () => { throw new Error("publish response lost"); },
});
assert.equal((await promoteVerifiedChannel(request({ policy: policy({ attempts: 2 }) }), ambiguous.operations)).publication, "ambiguous");
assert.deepEqual(ambiguous.events, ["probe", "probe", "immutable", "promote", "probe", "probe", "default"]);

const noProof = operations([{ state: "missing", version: target }]);
await rejects(promoteVerifiedChannel(request({ exactConsumerVerified: false }), noProof.operations), /exact consumer/);
assert.deepEqual(noProof.events, []);

const immutableMismatch = operations([{ state: "present", version: "1.1.0" }], { verifyImmutable: () => { throw new Error("archive bytes mismatch"); } });
await rejects(promoteVerifiedChannel(request(), immutableMismatch.operations), /archive bytes mismatch/);
assert.deepEqual(immutableMismatch.events, ["probe", "immutable"]);
const immutableFalse = operations([{ state: "present", version: "1.1.0" }], { verifyImmutable: () => false });
await rejects(promoteVerifiedChannel(request(), immutableFalse.operations), /did not verify/);
assert.deepEqual(immutableFalse.events, ["probe", "immutable"]);

const defaultMismatch = operations([{ state: "present", version: target }], { verifyDefault: () => { throw new Error("default bytes mismatch"); } });
await rejects(promoteVerifiedChannel(request(), defaultMismatch.operations), /default bytes mismatch/);
assert.deepEqual(defaultMismatch.events, ["probe", "default"]);
const defaultUnknown = operations([{ state: "present", version: target }], { verifyDefault: () => { throw new Error("default endpoint unavailable"); } });
await rejects(promoteVerifiedChannel(request(), defaultUnknown.operations), /endpoint unavailable/);

for (const observed of ["1.2.1", "1.2.0+other"]) {
  const rollback = operations([{ state: "present", version: observed }]);
  await rejects(promoteVerifiedChannel(request(), rollback.operations), /observed version|not the exact/);
  assert.deepEqual(rollback.events, ["probe"]);
}
const stableOverPrerelease = operations([{ state: "present", version: "1.2.0" }]);
await rejects(promoteVerifiedChannel(request({ version: "1.2.0-rc.1" }), stableOverPrerelease.operations), /observed version/);

const malformedProbe = operations([], { probe: async () => ({ state: "present", version: "invalid" }) });
await rejects(promoteVerifiedChannel(request(), malformedProbe.operations), /semantic version/);
assert.deepEqual(malformedProbe.events, []);
const unknownProbe = operations([], { probe: async () => ({ state: "unknown", version: target }) });
await rejects(promoteVerifiedChannel(request(), unknownProbe.operations), /unknown/);

const permanentProbe = operations([], { probe: async () => { throw new Error("authorization denied"); } });
await rejects(promoteVerifiedChannel(request({ policy: policy({ attempts: 4 }) }), permanentProbe.operations), /authorization denied/);
assert.deepEqual(permanentProbe.events, []);

const exhausted = operations([{ state: "missing", version: target }]);
await rejects(promoteVerifiedChannel(request({ policy: policy({ deadline: performance.now() - 1 }) }), exhausted.operations), /deadline/);
assert.deepEqual(exhausted.events, []);

const ambiguousMissing = operations([{ state: "present", version: "1.1.0" }, { state: "missing", version: target }, { state: "missing", version: target }], {
  promote: () => { throw new Error("publish response lost"); },
});
await rejects(promoteVerifiedChannel(request({ policy: policy({ attempts: 2 }) }), ambiguousMissing.operations), /not observed|publish response/);
assert.equal(ambiguousMissing.events.filter((event) => event === "promote").length, 1);

const temporary = operations([new Error("ECONNRESET"), { state: "present", version: "1.1.0" }, new Error("E503"), { state: "present", version: target }]);
assert.equal((await promoteVerifiedChannel(request(), temporary.operations)).publication, "promoted");
assert.equal(temporary.events.filter((event) => event === "promote").length, 1);

for (const badVersion of ["1.2", "v1.2.0", "1.2.0-01"]) {
  const bad = operations([]);
  await rejects(promoteVerifiedChannel(request({ version: badVersion }), bad.operations), /semantic version|leading zero/);
  assert.deepEqual(bad.events, []);
}

const invalidPolicy = operations([]);
await rejects(promoteVerifiedChannel(request({ policy: policy({ attempts: 0 }) }), invalidPolicy.operations), /attempts/);
assert.deepEqual(invalidPolicy.events, []);

console.log("channel_promotion.test: OK (proof ordering, rollback, ambiguity, bounded retries, verification mismatches)");
