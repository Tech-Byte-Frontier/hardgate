// Behavioral contract for GitHub default-channel promotion.
"use strict";

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { performance } from "node:perf_hooks";
import { projectRoot } from "../scripts/release-support.mjs";
import { expectedGithubAssets } from "../scripts/github-staging-state.mjs";
import { promoteGithubChannel } from "../scripts/github-channel-promotion.mjs";
import {
  CHANNELS,
  RECEIPT_STATES,
  REQUIRED_CHANNELS,
  createReceipt,
  recordTransition,
  readReceipt,
  writeReceiptAtomicSync,
} from "../scripts/release-receipt.mjs";
import "../scripts/promote-github-channel.mjs";

const version = "1.2.3";
const assets = expectedGithubAssets(version);
const h40 = (letter) => letter.repeat(40);
const h64 = (letter) => letter.repeat(64);
const policy = () => ({ attempts: 3, delayMs: 0, deadline: performance.now() + 10_000 });
const clone = (value) => JSON.parse(JSON.stringify(value));

function identity(digests = assets.map((_, index) => h64(String(index % 10)))) {
  return {
    version,
    source_sha: h40("a"),
    tooling_sha: h40("b"),
    signed_tag_object: h40("c"),
    build_run_id: "100",
    artifact_id: "200",
    archives: [...assets].sort().map((name, index) => ({ name, sha256: digests[assets.indexOf(name)] })),
  };
}

function evidence(receipt, consumer = false) {
  const result = { version, source_sha: receipt.identity.source_sha, archives: clone(receipt.identity.archives) };
  if (consumer) result.consumer = { executable: "hardgate", sha256: h64("e") };
  return result;
}

function advanceTo(receipt, channel, target) {
  const stop = RECEIPT_STATES.indexOf(target);
  for (let index = 0; index < stop; index += 1) {
    const from = RECEIPT_STATES[index];
    const to = RECEIPT_STATES[index + 1];
    recordTransition(receipt, { channel, from, to, evidence: evidence(receipt, to === "exact_consumer_verified" || to === "default_consumer_verified") });
  }
  return receipt;
}

function gatedReceipt({ npm = "promoted", crate = "exact_consumer_verified", github = "exact_consumer_verified" } = {}) {
  const receipt = createReceipt(identity());
  for (const channel of CHANNELS.npmPlatforms) advanceTo(receipt, channel, npm);
  advanceTo(receipt, CHANNELS.npmWrapper, npm);
  advanceTo(receipt, CHANNELS.crate, crate);
  advanceTo(receipt, CHANNELS.githubAssets, github);
  return receipt;
}

function fakeOperations(sequence, overrides = {}) {
  const events = [];
  const probes = [...sequence];
  const nextProbe = async (request) => {
    events.push(["probe", request.exactConsumerVerified]);
    const value = probes.shift();
    if (value instanceof Error) throw value;
    return value ?? { state: "present", version };
  };
  return {
    events,
    operations: {
      probe: overrides.probe ?? nextProbe,
      verifyImmutable: async () => { events.push(["immutable"]); return overrides.verifyImmutable?.(); },
      verifyDefault: async () => { events.push(["default"]); return overrides.verifyDefault?.(); },
      promote: async () => { events.push(["promote"]); return overrides.promote?.(); },
    },
  };
}

async function rejects(action, pattern) {
  await assert.rejects(action, pattern);
}

const blocked = fakeOperations([{ state: "missing" }]);
await rejects(promoteGithubChannel({ receipt: gatedReceipt({ npm: "exact_consumer_verified" }), policy: policy() }, blocked.operations), /npm channels/);
assert.deepEqual(blocked.events, []);

const rollbackReceipt = gatedReceipt();
const rollback = fakeOperations([{ state: "present", version: "1.2.4" }]);
await rejects(promoteGithubChannel({ receipt: rollbackReceipt, policy: policy() }, rollback.operations), /observed version/);
assert.deepEqual(rollback.events, [["probe", true]]);
assert.equal(rollbackReceipt.channels[CHANNELS.githubAssets].state, "exact_consumer_verified");

const existingReceipt = gatedReceipt();
const existing = fakeOperations([{ state: "present", version }, { state: "present", version }]);
const existingResult = await promoteGithubChannel({ receipt: existingReceipt, policy: policy() }, existing.operations);
assert.equal(existingResult.publication, "existing");
assert.equal(existingResult.state, "promoted");
assert.deepEqual(existing.events.map(([name]) => name), ["probe", "immutable", "default", "probe"]);
assert.equal(existingReceipt.channels[CHANNELS.githubAssets].state, "promoted");

const ambiguousReceipt = gatedReceipt();
const ambiguous = fakeOperations([{ state: "present", version: "1.1.0" }, { state: "present", version }], { promote: () => { throw new Error("edit response lost"); } });
const ambiguousResult = await promoteGithubChannel({ receipt: ambiguousReceipt, policy: policy() }, ambiguous.operations);
assert.equal(ambiguousResult.publication, "ambiguous");
assert.equal(ambiguous.events.filter(([name]) => name === "promote").length, 1);
assert.equal(ambiguousReceipt.channels[CHANNELS.githubAssets].state, "promoted");

const defaultReceipt = gatedReceipt({ github: "default_consumer_verified" });
const defaultBefore = clone(defaultReceipt);
const alreadyDefault = fakeOperations([{ state: "present", version }, { state: "present", version }]);
const defaultResult = await promoteGithubChannel({ receipt: defaultReceipt, policy: policy() }, alreadyDefault.operations);
assert.equal(defaultResult.publication, "existing");
assert.equal(defaultResult.state, "default_consumer_verified");
assert.deepEqual(defaultReceipt, defaultBefore);

const immutableFailureReceipt = gatedReceipt();
const immutableFailure = fakeOperations([{ state: "missing" }], { verifyImmutable: () => { throw new Error("remote asset mismatch"); } });
await rejects(promoteGithubChannel({ receipt: immutableFailureReceipt, policy: policy() }, immutableFailure.operations), /remote asset mismatch/);
assert.equal(immutableFailureReceipt.channels[CHANNELS.githubAssets].state, "exact_consumer_verified");

assert.equal(REQUIRED_CHANNELS.length, 7);

const fixture = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-github-promotion-test-"));
try {
  const dist = path.join(fixture, "dist");
  const bin = path.join(fixture, "bin");
  const stateFile = path.join(fixture, "state.json");
  const logFile = path.join(fixture, "calls.jsonl");
  fs.mkdirSync(dist);
  fs.mkdirSync(bin);
  for (const name of assets) fs.writeFileSync(path.join(dist, name), "github fixture:" + name + "\n");
  const digests = assets.map((name) => crypto.createHash("sha256").update(fs.readFileSync(path.join(dist, name))).digest("hex"));
  const receipt = path.join(fixture, "receipt.json");
  writeReceiptAtomicSync(receipt, gatedReceiptWithDigests(digests), identity(digests));
  fs.writeFileSync(stateFile, JSON.stringify({ latest: "missing", targetPrerelease: true, editCount: 0, mismatch: null, duplicateAssets: false, transientProbeFailures: 0 }));
  const fakeGh = [
    "#!/usr/bin/env node",
    "const fs=require(\"node:fs\"); const path=require(\"node:path\");",
    "const args=process.argv.slice(2); const stateFile=" + JSON.stringify(stateFile) + "; const logFile=" + JSON.stringify(logFile) + "; const dist=" + JSON.stringify(dist) + ";",
    "const state=JSON.parse(fs.readFileSync(stateFile,\"utf8\"));",
    "const log=(kind)=>fs.appendFileSync(logFile,JSON.stringify({kind,args,env:Object.fromEntries([\"GH_TOKEN\",\"GH_HOST\",\"GITHUB_TOKEN\",\"NPM_TOKEN\",\"CARGO_REGISTRY_TOKEN\",\"ACTIONS_ID_TOKEN_REQUEST_TOKEN\"].map(k=>[k,process.env[k]??null]))})+\"\\n\");",
    "const fail=(text)=>{process.stderr.write(text+\"\\n\");process.exit(1);}; const save=()=>fs.writeFileSync(stateFile,JSON.stringify(state));",
    "log(args[0]+\"-\"+args[1]);",
    "if(args[0]===\"api\"){if(state.transientProbeFailures>0){state.transientProbeFailures-=1;save();fail(\"HTTP 503: transient\");}if(state.latest===\"missing\")fail(\"HTTP 404: Not Found\");if(state.latest===\"error\")fail(\"HTTP 500: Internal Server Error\");if(state.latest===\"credential\")fail(\"credential-like sentinel\");if(state.latest===\"unauthorized\")fail(\"HTTP 401: Unauthorized\");if(state.latest===\"malformed\"){process.stdout.write(\"not-json\");}else process.stdout.write(JSON.stringify({tag_name:state.latest,draft:false,prerelease:false}));}",
    "else if(args[0]===\"release\"&&args[1]===\"view\"){const names=fs.readdirSync(dist);if(state.duplicateAssets)names[1]=names[0];process.stdout.write(JSON.stringify({tagName:state.viewTag||\"v1.2.3\",isDraft:false,isPrerelease:state.targetPrerelease,assets:names.map(name=>({name}))}));}",
    "else if(args[0]===\"release\"&&args[1]===\"download\"){const name=args[args.indexOf(\"--pattern\")+1];const directory=args[args.indexOf(\"--dir\")+1];if(state.mismatch===name)fs.writeFileSync(path.join(directory,name),\"wrong bytes\");else fs.copyFileSync(path.join(dist,name),path.join(directory,name));}",
    "else if(args[0]===\"release\"&&args[1]===\"edit\"){state.editCount+=1;state.latest=\"v1.2.3\";state.targetPrerelease=false;save();}",
    "else fail(\"unexpected gh command\");",
  ].join("\n");
  fs.writeFileSync(path.join(bin, "gh"), fakeGh, { mode: 0o755 });
  const runCli = (receiptPath = receipt, prefix = []) => spawnSync(process.execPath, [path.join(projectRoot, "scripts/promote-github-channel.mjs"), ...prefix, "--receipt", receiptPath, "--dist", dist, "--repo", "owner/repo"], {
    cwd: projectRoot,
    encoding: "utf8",
    env: { ...process.env, PATH: bin + path.delimiter + process.env.PATH, GH_TOKEN: "fixture-token", GITHUB_TOKEN: "must-not-leak", NPM_TOKEN: "must-not-leak", CARGO_REGISTRY_TOKEN: "must-not-leak", ACTIONS_ID_TOKEN_REQUEST_TOKEN: "must-not-leak" },
  });
  const promoted = runCli();
  assert.equal(promoted.status, 0, promoted.stderr);
  assert.match(promoted.stdout, /promoted/);
  const calls = fs.readFileSync(logFile, "utf8").trim().split("\n").map(JSON.parse);
  assert.equal(calls.filter((call) => call.kind === "release-edit").length, 1);
  assert.equal(calls[0].env.GH_HOST, "github.com");
  assert.equal(calls[0].env.GH_TOKEN, "fixture-token");
  for (const name of ["GITHUB_TOKEN", "NPM_TOKEN", "CARGO_REGISTRY_TOKEN", "ACTIONS_ID_TOKEN_REQUEST_TOKEN"]) assert.equal(calls[0].env[name], null, name);
  const promotedReceipt = readReceipt(receipt);
  assert.equal(promotedReceipt.channels[CHANNELS.githubAssets].state, "promoted");
  const stableBytes = fs.readFileSync(receipt);
  assert.equal(runCli().status, 0);
  assert.deepEqual(fs.readFileSync(receipt), stableBytes);
  assert.equal(JSON.parse(fs.readFileSync(stateFile, "utf8")).editCount, 1);

  const transientReceipt = path.join(fixture, "transient.json");
  writeReceiptAtomicSync(transientReceipt, gatedReceiptWithDigests(digests), identity(digests));
  fs.writeFileSync(stateFile, JSON.stringify({ latest: "missing", targetPrerelease: true, editCount: 0, mismatch: null, duplicateAssets: false, transientProbeFailures: 1 }));
  const transient = runCli(transientReceipt);
  assert.equal(transient.status, 0, transient.stderr);
  assert.equal(JSON.parse(fs.readFileSync(stateFile, "utf8")).editCount, 1);

  const callsBeforeBadOption = fs.readFileSync(logFile);
  const badOption = runCli(receipt, ["xxreceipt"]);
  assert.notEqual(badOption.status, 0);
  assert.deepEqual(fs.readFileSync(logFile), callsBeforeBadOption);
  assert.doesNotMatch(badOption.stderr, /credential|sentinel/i);

  const localMismatchReceipt = path.join(fixture, "local-mismatch.json");
  writeReceiptAtomicSync(localMismatchReceipt, gatedReceiptWithDigests(digests), identity(digests));
  const localAsset = path.join(dist, assets[0]);
  const localBytes = fs.readFileSync(localAsset);
  const callsBeforeLocalMismatch = fs.readFileSync(logFile);
  fs.writeFileSync(localAsset, "local bytes mismatch");
  assert.notEqual(runCli(localMismatchReceipt).status, 0);
  assert.deepEqual(fs.readFileSync(logFile), callsBeforeLocalMismatch);
  fs.writeFileSync(localAsset, localBytes);

  const mismatchReceipt = path.join(fixture, "mismatch.json");
  writeReceiptAtomicSync(mismatchReceipt, gatedReceiptWithDigests(digests), identity(digests));
  fs.writeFileSync(stateFile, JSON.stringify({ latest: "missing", targetPrerelease: true, editCount: 0, mismatch: assets[0], duplicateAssets: false, transientProbeFailures: 0 }));
  const mismatch = runCli(mismatchReceipt);
  assert.notEqual(mismatch.status, 0);
  assert.equal(JSON.parse(fs.readFileSync(stateFile, "utf8")).editCount, 0);
  assert.equal(readReceipt(mismatchReceipt).channels[CHANNELS.githubAssets].events.at(-1).type, "failure");

  const duplicateReceipt = path.join(fixture, "duplicate.json");
  writeReceiptAtomicSync(duplicateReceipt, gatedReceiptWithDigests(digests), identity(digests));
  fs.writeFileSync(stateFile, JSON.stringify({ latest: "missing", targetPrerelease: true, editCount: 0, mismatch: null, duplicateAssets: true, transientProbeFailures: 0 }));
  const duplicate = runCli(duplicateReceipt);
  assert.notEqual(duplicate.status, 0);
  assert.equal(JSON.parse(fs.readFileSync(stateFile, "utf8")).editCount, 0);

  const errorReceipt = path.join(fixture, "error.json");
  writeReceiptAtomicSync(errorReceipt, gatedReceiptWithDigests(digests), identity(digests));
  const callsBeforeError = fs.readFileSync(logFile, "utf8").split("\n").filter(Boolean).length;
  fs.writeFileSync(stateFile, JSON.stringify({ latest: "error", targetPrerelease: true, editCount: 0, mismatch: null, duplicateAssets: false, transientProbeFailures: 0 }));
  const errorResult = runCli(errorReceipt);
  assert.notEqual(errorResult.status, 0);
  assert.equal(JSON.parse(fs.readFileSync(stateFile, "utf8")).editCount, 0);
  const callsAfterError = fs.readFileSync(logFile, "utf8").split("\n").filter(Boolean).length;
  assert.equal(callsAfterError - callsBeforeError, 3);
  assert.match(errorResult.stderr, /github-promotion-failed: GitHub channel promotion failed/);

  const unauthorizedReceipt = path.join(fixture, "unauthorized.json");
  writeReceiptAtomicSync(unauthorizedReceipt, gatedReceiptWithDigests(digests), identity(digests));
  const callsBeforeUnauthorized = fs.readFileSync(logFile, "utf8").split("\n").filter(Boolean).length;
  fs.writeFileSync(stateFile, JSON.stringify({ latest: "unauthorized", targetPrerelease: true, editCount: 0, mismatch: null, duplicateAssets: false, transientProbeFailures: 0 }));
  const unauthorized = runCli(unauthorizedReceipt);
  assert.notEqual(unauthorized.status, 0);
  const callsAfterUnauthorized = fs.readFileSync(logFile, "utf8").split("\n").filter(Boolean).length;
  assert.equal(callsAfterUnauthorized - callsBeforeUnauthorized, 1);

  const malformedReceipt = path.join(fixture, "malformed.json");
  writeReceiptAtomicSync(malformedReceipt, gatedReceiptWithDigests(digests), identity(digests));
  const callsBeforeMalformed = fs.readFileSync(logFile, "utf8").split("\n").filter(Boolean).length;
  fs.writeFileSync(stateFile, JSON.stringify({ latest: "malformed", targetPrerelease: true, editCount: 0, mismatch: null, duplicateAssets: false, transientProbeFailures: 0 }));
  const malformed = runCli(malformedReceipt);
  assert.notEqual(malformed.status, 0);
  const callsAfterMalformed = fs.readFileSync(logFile, "utf8").split("\n").filter(Boolean).length;
  assert.equal(callsAfterMalformed - callsBeforeMalformed, 1);

  const credentialReceipt = path.join(fixture, "credential.json");
  writeReceiptAtomicSync(credentialReceipt, gatedReceiptWithDigests(digests), identity(digests));
  fs.writeFileSync(stateFile, JSON.stringify({ latest: "credential", targetPrerelease: true, editCount: 0, mismatch: null, duplicateAssets: false, transientProbeFailures: 0 }));
  const credential = runCli(credentialReceipt);
  assert.notEqual(credential.status, 0);
  assert.doesNotMatch(credential.stdout, /credential-like sentinel/);
  assert.doesNotMatch(credential.stderr, /credential-like sentinel/);
  assert.doesNotMatch(fs.readFileSync(credentialReceipt, "utf8"), /credential-like sentinel/);

  const wrongTagReceipt = path.join(fixture, "wrong-tag.json");
  writeReceiptAtomicSync(wrongTagReceipt, gatedReceiptWithDigests(digests), identity(digests));
  fs.writeFileSync(stateFile, JSON.stringify({ latest: "missing", targetPrerelease: true, editCount: 0, mismatch: null, viewTag: "v9.9.9", duplicateAssets: false, transientProbeFailures: 0 }));
  const wrongTag = runCli(wrongTagReceipt);
  assert.notEqual(wrongTag.status, 0);
  assert.equal(JSON.parse(fs.readFileSync(stateFile, "utf8")).editCount, 0);
} finally {
  fs.rmSync(fixture, { recursive: true, force: true });
}

function gatedReceiptWithDigests(digests) {
  const receipt = createReceipt(identity(digests));
  for (const channel of CHANNELS.npmPlatforms) advanceTo(receipt, channel, "promoted");
  advanceTo(receipt, CHANNELS.npmWrapper, "promoted");
  advanceTo(receipt, CHANNELS.crate, "exact_consumer_verified");
  advanceTo(receipt, CHANNELS.githubAssets, "exact_consumer_verified");
  return receipt;
}

console.log("github_channel_promotion.test: OK (receipt gates, immutable proof, stable preservation, ambiguity, CLI isolation)");
