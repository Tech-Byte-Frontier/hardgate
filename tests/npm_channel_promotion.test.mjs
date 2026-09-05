// Contract tests for bounded npm latest-channel promotion.
"use strict";
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";
import { CHANNELS, REQUIRED_CHANNELS, createReceipt, readReceipt, recordTransition, writeReceiptAtomicSync } from "../scripts/release-receipt.mjs";
import { NPM_CHANNELS, credentialFreeEnvironment, latestUrl, probeNpmLatest } from "../scripts/npm-channel-promotion.mjs";
import { promoteNpmChannels, run } from "../scripts/promote-npm-channels.mjs";

const version = "1.2.3";
const sourceCwd = path.resolve(".");
const toolingVerifier = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../scripts/verify-npm-publication.mjs");
const h40 = (letter) => letter.repeat(40);
const h64 = (letter) => letter.repeat(64);
const policy = (overrides = {}) => ({ attempts: 2, delayMs: 0, childMs: 1000, deadline: performance.now() + 20_000, ...overrides });

function evidence(identity, consumer = true) {
  const value = { version: identity.version, source_sha: identity.source_sha, archives: identity.archives.map((item) => ({ ...item })) };
  if (consumer) value.consumer = { executable: "/tmp/hardgate-test-consumer", sha256: h64("e") };
  return value;
}
function fixture() {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-npm-promotion-test-"));
  const dist = path.join(directory, "dist");
  fs.mkdirSync(dist);
  const names = ["hardgate-linux-x64.tar.gz", "hardgate-linux-x64-musl.tar.gz", "hardgate-linux-arm64.tar.gz", "hardgate-linux-arm64-musl.tar.gz", "hardgate-darwin-x64.tar.gz", "hardgate-darwin-arm64.tar.gz", "SHA256SUMS", `hardgate-${version}.sbom.cdx.json`].sort();
  const archives = names.map((name, index) => {
    const bytes = Buffer.from(`release archive ${index} ${name}\n`);
    fs.writeFileSync(path.join(dist, name), bytes);
    return { name, sha256: crypto.createHash("sha256").update(bytes).digest("hex") };
  });
  const identity = { version, source_sha: h40("1"), tooling_sha: h40("2"), signed_tag_object: h40("3"), build_run_id: "10", artifact_id: "20", archives };
  const receiptPath = path.join(directory, "release-receipt.json");
  const receipt = createReceipt(identity);
  for (const channel of REQUIRED_CHANNELS) {
    recordTransition(receipt, { channel, from: "pending", to: "staged", evidence: evidence(identity, false) });
    recordTransition(receipt, { channel, from: "staged", to: "immutable_verified", evidence: evidence(identity, false) });
    recordTransition(receipt, { channel, from: "immutable_verified", to: "exact_consumer_verified", evidence: evidence(identity) });
  }
  writeReceiptAtomicSync(receiptPath, receipt, identity);
  return { directory, dist, identity, receiptPath };
}
async function withFixture(action) {
  const value = fixture();
  try { return await action(value); } finally { fs.rmSync(value.directory, { recursive: true, force: true }); }
}
function scopedConfig(options, promotion = false) {
  const { env } = options;
  assert.ok(env.HOME && env.npm_config_cache && env.npm_config_userconfig && env.npm_config_globalconfig);
  assert.equal(env.NPM_CONFIG_USERCONFIG, undefined);
  assert.equal(env.NPM_CONFIG_GLOBALCONFIG, undefined);
  assert.equal(fs.statSync(env.npm_config_userconfig).mode & 0o777, 0o600);
  assert.equal(fs.statSync(env.npm_config_globalconfig).mode & 0o777, 0o600);
  const userConfig = fs.readFileSync(env.npm_config_userconfig, "utf8");
  assert.equal(fs.readFileSync(env.npm_config_globalconfig, "utf8"), "");
  assert.match(userConfig, /registry=https:\/\/registry\.npmjs\.org/);
  if (promotion) {
    assert.match(userConfig, /:_authToken=\$\{NODE_AUTH_TOKEN\}/);
    assert.equal(userConfig.includes(env.NODE_AUTH_TOKEN), false);
    assert.ok(env.NODE_AUTH_TOKEN);
  } else {
    assert.equal(userConfig.includes("_authToken"), false);
    assert.equal(env.NODE_AUTH_TOKEN, undefined);
  }
}
function runner(states, events, { failMutation = false, mutationMakesTarget = true } = {}) {
  return async (command, args, options) => {
    events.push({ type: "run", command, args: [...args], options });
    assert.ok(options.timeoutMs > 0);
    if (command === process.execPath) assert.ok(options.timeoutMs > 1000);
    scopedConfig(options, command === "npm");
    if (command === "npm") {
      const spec = args[2];
      const name = spec.slice(0, spec.lastIndexOf("@"));
      if (mutationMakesTarget) states[name] = version;
      if (failMutation) throw Object.assign(new Error("registry token leaked: SHOULD_NOT_PRINT"), { code: "EPIPE" });
    } else if (command === process.execPath) {
      assert.equal(args[0], toolingVerifier);
    }
    return "";
  };
}
function mutatingRunner(states, events, mutate) {
  let mutations = 0;
  const baseRunner = runner(states, events);
  const runProcess = async (command, args, options) => {
    const result = await baseRunner(command, args, options);
    if (command === "npm") {
      mutations += 1;
      mutate({ args, count: mutations });
    }
    return result;
  };
  return { runProcess, mutationCount: () => mutations };
}
function probe(states, events, scripted = new Map()) {
  return async ({ name, version: requested, env }) => {
    events.push({ type: "probe", name, env });
    for (const key of ["NODE_AUTH_TOKEN", "NPM_TOKEN", "NPM_PROMOTION_TOKEN", "GITHUB_TOKEN", "GH_TOKEN", "ACTIONS_ID_TOKEN_REQUEST_TOKEN", "ACTIONS_ID_TOKEN_REQUEST_URL"]) assert.equal(env[key], undefined, key);
    const queue = scripted.get(name);
    const observed = queue?.length ? queue.shift() : states[name];
    if (observed instanceof Error) throw observed;
    if (observed === "missing") return { state: "missing" };
    return { state: "present", metadata: { name, version: observed ?? requested } };
  };
}

function testEnvironment() {
  const source = { PATH: "/safe/bin", NODE_AUTH_TOKEN: "publish-secret", NPM_TOKEN: "other-secret", NPM_PROMOTION_TOKEN: "promotion-secret", GITHUB_TOKEN: "github-secret", GH_TOKEN: "gh-secret", ACTIONS_ID_TOKEN_REQUEST_TOKEN: "oidc-secret", ACTIONS_ID_TOKEN_REQUEST_URL: "https://actions.example.invalid/token", NPM_CONFIG_USERCONFIG: "ambient-user-config", NPM_CONFIG_GLOBALCONFIG: "ambient-global-config", npm_config_userconfig: "ambient-lower-user-config", npm_config_globalconfig: "ambient-lower-global-config", npm_config__auth: "config-secret" };
  assert.deepEqual(credentialFreeEnvironment(source), { PATH: "/safe/bin" });
  assert.equal(source.NODE_AUTH_TOKEN, "publish-secret");
  assert.equal(latestUrl("@tech-byte-frontier/hardgate"), "https://registry.npmjs.org/%40tech-byte-frontier%2Fhardgate/latest");
}

async function testHappyPath() {
  await withFixture(async ({ dist, receiptPath }) => {
    const states = Object.fromEntries(NPM_CHANNELS.map((name) => [name, "1.2.2"]));
    const events = [];
    const env = { PATH: "/safe/bin", NODE_AUTH_TOKEN: "promotion-secret", NPM_TOKEN: "unrelated-secret", GITHUB_TOKEN: "github-secret", ACTIONS_ID_TOKEN_REQUEST_TOKEN: "oidc-secret", ACTIONS_ID_TOKEN_REQUEST_URL: "https://actions.example.invalid/token" };
    const result = await promoteNpmChannels({ receiptPath, distDir: dist, sourceCwd, env, policy: policy(), runProcess: runner(states, events), probeLatest: probe(states, events) });
    assert.deepEqual(result.results.map((item) => item.channel), NPM_CHANNELS);
    assert.equal(events.filter((item) => item.type === "run" && item.command === "npm").length, 7);
    assert.equal(events.filter((item) => item.type === "run" && item.command === process.execPath).length, 7);
    const mutation = events.find((item) => item.type === "run" && item.command === "npm");
    assert.deepEqual(mutation.args.slice(0, 4), ["dist-tag", "add", "hardgate-linux-x64@1.2.3", "latest"]);
    assert.equal(mutation.options.env.NODE_AUTH_TOKEN, "promotion-secret");
    assert.equal(mutation.options.env.GITHUB_TOKEN, undefined);
    const platform = events.find((item) => item.type === "run" && item.command === process.execPath && item.args.includes("hardgate-linux-x64"));
    assert.deepEqual(platform.args.slice(0, 5), [toolingVerifier, "--version", version, "--dist", path.resolve(dist)]);
    assert.deepEqual(platform.args.slice(-3), ["--platform-only", "--package", "hardgate-linux-x64"]);
    const wrapper = events.find((item) => item.type === "run" && item.command === process.execPath && !item.args.includes("--platform-only"));
    assert.equal(wrapper.args.includes("--package"), false);
    assert.equal(platform.options.env.NODE_AUTH_TOKEN, undefined);
    assert.equal(wrapper.options.env.GITHUB_TOKEN, undefined);
    for (const event of events.filter((item) => item.type === "run")) {
      assert.equal(fs.existsSync(event.options.env.npm_config_userconfig), false);
      assert.equal(fs.existsSync(event.options.env.npm_config_globalconfig), false);
      assert.equal(fs.existsSync(event.options.env.HOME), false);
      assert.equal(fs.existsSync(event.options.env.npm_config_cache), false);
    }
    const saved = readReceipt(receiptPath);
    for (const channel of NPM_CHANNELS) {
      assert.equal(saved.channels[channel].state, "promoted", channel);
      assert.equal(saved.channels[channel].events.filter((item) => item.type === "transition" && item.to === "promoted").length, 1);
      assert.equal(saved.channels[channel].events.some((item) => item.to === "default_consumer_verified"), false);
    }
    assert.equal(saved.channels[CHANNELS.crate].state, "exact_consumer_verified");
    assert.equal(saved.channels[CHANNELS.githubAssets].state, "exact_consumer_verified");
  });
}

async function testToolingVerifierAuthority() {
  await withFixture(async ({ directory, dist, receiptPath }) => {
    const stale = path.join(directory, "scripts", "verify-npm-publication.mjs");
    fs.mkdirSync(path.dirname(stale), { recursive: true });
    fs.writeFileSync(stale, "throw new Error('stale signed-source verifier');\n");
    const states = Object.fromEntries(NPM_CHANNELS.map((name) => [name, "1.2.2"]));
    const events = [];
    const baseRunner = runner(states, events);
    const runProcess = async (command, args, options) => {
      if (command === process.execPath) {
        assert.equal(options.cwd, directory);
        if (args[0] === stale) throw new Error("stale signed-source verifier was selected");
        assert.equal(args[0], toolingVerifier);
        assert.match(fs.readFileSync(stale, "utf8"), /stale signed-source verifier/);
      }
      return baseRunner(command, args, options);
    };
    await run(["--receipt", receiptPath, "--dist", dist], { sourceCwd: directory, env: { NODE_AUTH_TOKEN: "promotion-secret" }, policy: policy(), runProcess, probeLatest: probe(states, events) });
    assert.equal(events.filter((item) => item.type === "run" && item.command === process.execPath).length, 7);
  });
}

async function testGates() {
  await withFixture(async ({ dist, receiptPath, identity }) => {
    const incomplete = fixture();
    try {
      const pending = readReceipt(incomplete.receiptPath);
      pending.channels[CHANNELS.crate] = { state: "pending", events: [] };
      writeReceiptAtomicSync(incomplete.receiptPath, pending, incomplete.identity);
      let calls = 0;
      await assert.rejects(promoteNpmChannels({ receiptPath: incomplete.receiptPath, distDir: incomplete.dist, sourceCwd, policy: policy(), probeLatest: async () => { calls += 1; return { state: "missing" }; }, runProcess: async () => { calls += 1; return ""; } }), /exact consumer verification/);
      assert.equal(calls, 0);
    } finally { fs.rmSync(incomplete.directory, { recursive: true, force: true }); }
    const extra = path.join(dist, "unexpected.txt");
    fs.writeFileSync(extra, "extra\n");
    let calls = 0;
    await assert.rejects(promoteNpmChannels({ receiptPath, distDir: dist, sourceCwd, policy: policy(), probeLatest: async () => { calls += 1; return { state: "missing" }; }, runProcess: async () => { calls += 1; return ""; } }), /exactly the receipt/);
    assert.equal(calls, 0);
    fs.rmSync(extra);
    const link = path.join(dist, "unexpected-link");
    fs.symlinkSync(identity.archives[0].name, link);
    calls = 0;
    await assert.rejects(promoteNpmChannels({ receiptPath, distDir: dist, sourceCwd, policy: policy(), probeLatest: async () => { calls += 1; return { state: "missing" }; }, runProcess: async () => { calls += 1; return ""; } }), /exactly the receipt/);
    assert.equal(calls, 0);
    fs.rmSync(link);
    const altered = path.join(dist, identity.archives[0].name);
    fs.appendFileSync(altered, "tampered\n");
    calls = 0;
    await assert.rejects(promoteNpmChannels({ receiptPath, distDir: dist, sourceCwd, env: { NODE_AUTH_TOKEN: "secret" }, policy: policy(), probeLatest: async () => { calls += 1; return { state: "missing" }; }, runProcess: async () => { calls += 1; return ""; } }), /digest/);
    assert.equal(calls, 0);
  });
}

async function testMetadataAndNoMutation() {
  for (const mode of ["newer", "wrong-name"]) await withFixture(async ({ dist, receiptPath }) => {
    const events = [];
    const probeLatest = async ({ name }) => {
      events.push("probe");
      return { state: "present", metadata: { name: mode === "wrong-name" ? "other-package" : name, version: mode === "newer" ? "9.0.0" : version } };
    };
    await assert.rejects(promoteNpmChannels({ receiptPath, distDir: dist, sourceCwd, env: {}, policy: policy(), probeLatest, runProcess: async (command) => { if (command === "npm") throw new Error("mutation"); return ""; } }), mode === "newer" ? /newer/ : /wrong package name/);
    assert.deepEqual(events, ["probe"]);
    const saved = readReceipt(receiptPath);
    assert.equal(saved.channels[NPM_CHANNELS[0]].state, "exact_consumer_verified");
    assert.equal(saved.channels[NPM_CHANNELS[0]].events.at(-1).type, "failure");
  });
}

async function testStrictProbeAndAuth() {
  const name = NPM_CHANNELS[0];
  const call = async (output) => {
    let config;
    try {
      return await probeNpmLatest({ name, sourceCwd, policy: policy(), env: { NODE_AUTH_TOKEN: "secret", GITHUB_TOKEN: "secret" }, runProcess: async (command, args, options) => { assert.equal(command, "curl"); assert.equal(args.at(-1), latestUrl(name)); assert.equal(options.env.NODE_AUTH_TOKEN, undefined); assert.equal(options.env.GITHUB_TOKEN, undefined); config = options.env.npm_config_userconfig; scopedConfig(options); return output; } });
    } finally {
      assert.equal(fs.existsSync(config), false);
    }
  };
  assert.deepEqual(await call(`{"name":"${name}","version":"1.2.2"}\n200\n`), { state: "present", metadata: { name, version: "1.2.2" }, version: "1.2.2" });
  assert.deepEqual(await call("\n404\n"), { state: "missing" });
  await assert.rejects(call("{}\n401\n"), /fatal HTTP/);
  await assert.rejects(call("{}\n503\n"), /fatal HTTP/);
  await assert.rejects(call("not-json\n200\n"), /valid JSON/);
  await withFixture(async ({ dist, receiptPath }) => {
    const states = Object.fromEntries(NPM_CHANNELS.map((channel) => [channel, "1.2.2"]));
    const events = [];
    await assert.rejects(promoteNpmChannels({ receiptPath, distDir: dist, sourceCwd, env: { NPM_TOKEN: "wrong-secret" }, policy: policy(), runProcess: runner(states, events), probeLatest: probe(states, events) }), /NODE_AUTH_TOKEN/);
    assert.equal(events.filter((item) => item.type === "run" && item.command === "npm").length, 0);
    assert.equal(events.filter((item) => item.type === "run" && item.command === process.execPath).length, 1);
    assert.equal(readReceipt(receiptPath).channels[name].events.at(-1).code, "npm_auth_missing");
  });
}

async function testTransientAndAmbiguous() {
  await withFixture(async ({ dist, receiptPath }) => {
    const states = Object.fromEntries(NPM_CHANNELS.map((name) => [name, "1.2.2"]));
    const events = [];
    const scripted = new Map([[NPM_CHANNELS[0], [Object.assign(new Error("transient secret"), { retryable: true }), "1.2.2"]]]);
    await promoteNpmChannels({ receiptPath, distDir: dist, sourceCwd, env: { NODE_AUTH_TOKEN: "secret" }, policy: policy(), runProcess: runner(states, events), probeLatest: probe(states, events, scripted) });
    assert.equal(events.filter((item) => item.type === "run" && item.command === "npm").length, 7);
    assert.equal(JSON.stringify(readReceipt(receiptPath)).includes("transient secret"), false);
  });
  await withFixture(async ({ dist, receiptPath }) => {
    const states = Object.fromEntries(NPM_CHANNELS.map((name) => [name, "missing"]));
    const events = [];
    await promoteNpmChannels({ receiptPath, distDir: dist, sourceCwd, env: { NODE_AUTH_TOKEN: "secret" }, policy: policy(), runProcess: runner(states, events, { failMutation: true }), probeLatest: probe(states, events) });
    assert.equal(events.filter((item) => item.type === "run" && item.command === "npm").length, 7);
    assert.equal(JSON.stringify(readReceipt(receiptPath)).includes("SHOULD_NOT_PRINT"), false);
  });
}

async function testRevalidationBindsEachChannel() {
  await withFixture(async ({ dist, receiptPath, identity }) => {
    const states = Object.fromEntries(NPM_CHANNELS.map((name) => [name, "1.2.2"]));
    const events = [];
    const mutation = mutatingRunner(states, events, ({ count }) => {
      if (count === 1) fs.appendFileSync(path.join(dist, identity.archives[0].name), "changed after immutable proof\n");
    });
    await assert.rejects(promoteNpmChannels({ receiptPath, distDir: dist, sourceCwd, env: { NODE_AUTH_TOKEN: "secret" }, policy: policy(), runProcess: mutation.runProcess, probeLatest: probe(states, events) }), /immutable/);
    assert.equal(mutation.mutationCount(), 1);
    for (const event of events.filter((item) => item.type === "run")) {
      assert.equal(fs.existsSync(event.options.env.npm_config_userconfig), false);
      assert.equal(fs.existsSync(event.options.env.npm_config_globalconfig), false);
      assert.equal(fs.existsSync(event.options.env.HOME), false);
      assert.equal(fs.existsSync(event.options.env.npm_config_cache), false);
    }
    const saved = readReceipt(receiptPath);
    assert.equal(saved.channels[NPM_CHANNELS[0]].state, "exact_consumer_verified");
    assert.equal(saved.channels[NPM_CHANNELS[0]].events.at(-1).code, "npm_immutable_failed");
  });
}

async function testFinalChannelRevalidation() {
  await withFixture(async ({ dist, receiptPath, identity }) => {
    const states = Object.fromEntries(NPM_CHANNELS.map((name) => [name, "1.2.2"]));
    const events = [];
    const mutation = mutatingRunner(states, events, ({ args }) => {
      const spec = args[2];
      const name = spec.slice(0, spec.lastIndexOf("@"));
      if (name === CHANNELS.npmWrapper) fs.appendFileSync(path.join(dist, identity.archives[0].name), "changed after wrapper mutation\n");
    });
    await assert.rejects(promoteNpmChannels({ receiptPath, distDir: dist, sourceCwd, env: { NODE_AUTH_TOKEN: "secret" }, policy: policy(), runProcess: mutation.runProcess, probeLatest: probe(states, events) }), /immutable/);
    assert.equal(mutation.mutationCount(), NPM_CHANNELS.length);
    const saved = readReceipt(receiptPath);
    for (const channel of NPM_CHANNELS.slice(0, -1)) assert.equal(saved.channels[channel].state, "promoted", channel);
    assert.equal(saved.channels[CHANNELS.npmWrapper].state, "exact_consumer_verified");
    assert.equal(saved.channels[CHANNELS.npmWrapper].events.at(-1).code, "npm_immutable_failed");
  });
}

async function testReadbackAndPartialResume() {
  await withFixture(async ({ dist, receiptPath }) => {
    const states = Object.fromEntries(NPM_CHANNELS.map((name) => [name, "missing"]));
    const events = [];
    await assert.rejects(promoteNpmChannels({ receiptPath, distDir: dist, sourceCwd, env: { NODE_AUTH_TOKEN: "secret" }, policy: policy(), runProcess: runner(states, events, { mutationMakesTarget: false }), probeLatest: probe(states, events) }), /readback/);
    assert.equal(events.filter((item) => item.type === "run" && item.command === "npm").length, 1);
    const failure = readReceipt(receiptPath).channels[NPM_CHANNELS[0]].events.at(-1);
    assert.deepEqual({ code: failure.code, message: failure.message }, { code: "npm_readback_failed", message: "npm latest tag readback did not identify the requested release" });
  });
  await withFixture(async ({ dist, receiptPath, identity }) => {
    const receipt = readReceipt(receiptPath);
    for (const channel of NPM_CHANNELS.slice(0, 2)) recordTransition(receipt, { channel, from: "exact_consumer_verified", to: "promoted", evidence: evidence(identity, false) });
    writeReceiptAtomicSync(receiptPath, receipt, identity);
    const states = Object.fromEntries(NPM_CHANNELS.map((name) => [name, version]));
    const events = [];
    await promoteNpmChannels({ receiptPath, distDir: dist, sourceCwd, env: {}, policy: policy(), runProcess: runner(states, events, { mutationMakesTarget: false }), probeLatest: probe(states, events) });
    assert.equal(events.filter((item) => item.type === "run" && item.command === "npm").length, 0);
    const saved = readReceipt(receiptPath);
    for (const channel of NPM_CHANNELS) {
      assert.equal(saved.channels[channel].state, "promoted");
      assert.equal(saved.channels[channel].events.filter((item) => item.type === "transition" && item.to === "promoted").length, 1);
      assert.equal(saved.channels[channel].events.some((item) => item.to === "default_consumer_verified"), false);
    }
  });
}

testEnvironment();
await testHappyPath();
await testToolingVerifierAuthority();
await testGates();
await testMetadataAndNoMutation();
await testStrictProbeAndAuth();
await testTransientAndAmbiguous();
await testRevalidationBindsEachChannel();
await testFinalChannelRevalidation();
await testReadbackAndPartialResume();
console.log("npm_channel_promotion.test: OK (receipt/digest gates, scoped credentials, strict probes, immutable verification, one-shot mutation, retry, readback, resume, and fixed failures)");
