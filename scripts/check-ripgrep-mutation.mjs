#!/usr/bin/env node
// Real optional cargo-mutants acceptance for ripgrep's ByteSet::add example.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { runReleaseProcess } from "./release-process.mjs";

const OMITTED = new Set([".git", "target", ".hardgate"]);
const digest = (file) => crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
function inputs(root, directory = root, result = {}) {
  for (const item of fs.readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
    if (OMITTED.has(item.name)) continue;
    const file = path.join(directory, item.name);
    if (item.isSymbolicLink()) {
      const target = fs.realpathSync(file);
      assert(target.startsWith(`${root}${path.sep}`), "trial symlink escapes the checkout");
      result[path.relative(root, file)] = `symlink:${fs.readlinkSync(file)}`;
      continue;
    }
    if (item.isDirectory()) inputs(root, file, result);
    else if (item.isFile()) result[path.relative(root, file)] = digest(file);
  }
  return result;
}

async function run(root, output, label, command) {
  const start = performance.now();
  let result;
  try {
    const stdout = await runReleaseProcess(command[0], command.slice(1), { cwd: root, timeoutMs: 900_000 });
    result = { status: 0, stdout };
  } catch (error) {
    if (!Number.isInteger(error.status)) throw error;
    result = { status: error.status, stdout: error.stdout ?? "", stderr: error.stderr ?? "" };
  }
  fs.writeFileSync(path.join(output, `${label}.stdout`), result.stdout);
  fs.writeFileSync(path.join(output, `${label}.stderr`), result.stderr ?? "");
  const record = { command, exit: result.status, seconds: (performance.now() - start) / 1000 };
  fs.writeFileSync(path.join(output, `${label}.run.json`), JSON.stringify(record, null, 2) + "\n");
  console.log(`${label}: exit ${result.status} in ${record.seconds.toFixed(2)}s`);
  return result.status;
}

async function evidence(context, label, expected) {
  const { root, output, binary } = context;
  const selection = ["--package", "grep-matcher", "--file", "crates/matcher/src/lib.rs",
    "--re", String.raw`replace ByteSet::add with \(\)$`];
  const listing = JSON.parse(await runReleaseProcess("cargo", ["mutants", "--list", "--json", ...selection], { cwd: root, timeoutMs: 30_000 }));
  fs.writeFileSync(path.join(output, `${label}.listing.json`), JSON.stringify(listing, null, 2) + "\n");
  const identities = listing.map((item, index) => ({ item, index })).filter(({ item }) =>
    item.function?.function_name === "ByteSet::add" && item.replacement === "()");
  assert.equal(identities.length, 1, "the requested ByteSet mutation must be uniquely identified");
  // cargo-mutants 27.1.0 leaks struct-field deletions through --re. Select the
  // requested acceptance sample by its listed identity, retaining the full list.
  const command = [binary, "evidence", "cargo-mutants", "--", ...selection,
    "--shard", `${identities[0].index}/${listing.length}`, "--timeout", "300", "--offline"];
  const status = await run(root, output, label, command);
  assert.equal(status, expected === "MissedMutant" ? 1 : 0, `${label}: incomplete producer; inspect stderr`);
  const report = path.join(root, ".hardgate/evidence/mutation.json");
  const receipt = `${report}.hardgate.json`;
  for (const item of [report, receipt]) fs.copyFileSync(item, path.join(output, `${label}.${path.basename(item)}`));
  const data = JSON.parse(fs.readFileSync(report, "utf8"));
  const proof = JSON.parse(fs.readFileSync(receipt, "utf8"));
  assert(proof.restoration_verified && proof.prerequisite_passed);
  assert.equal(proof.report_sha256, digest(report));
  assert.equal(data.total_mutants, 1);
  assert.equal(data.timeout + data.unviable, 0);
  const selected = data.outcomes.filter((item) => item.scenario?.Mutant?.function?.function_name === "ByteSet::add"
    && item.scenario.Mutant.replacement === "()");
  assert.equal(selected.length, 1);
  assert.equal(selected[0].summary, expected);
  assert(data.outcomes.some((item) => item.scenario === "Baseline" && item.summary === "Success"));
  return { mutant: selected[0].scenario.Mutant, total: data.total_mutants };
}

async function exercise(context, test) {
    const { root, output } = context;
    const before = inputs(root);
    const first = await evidence(context, "survivor", "MissedMutant");
    assert.deepEqual(inputs(root), before);
    const originalTest = fs.readFileSync(test);
    try {
      fs.appendFileSync(test, "\n#[test]\nfn hardgate_trial_byteset_roundtrip() {\n    let mut bytes = grep_matcher::ByteSet::empty();\n    bytes.add(b'A');\n    assert!(bytes.contains(b'A'));\n}\n");
      assert.equal(await run(root, output, "original-assertion", ["cargo", "test", "--locked", "--offline", "-p", "grep-matcher", "hardgate_trial_byteset_roundtrip"]), 0);
      const tested = inputs(root);
      const second = await evidence(context, "killed", "CaughtMutant");
      assert.deepEqual(second, first, "the assertion must kill the same mutant in the same reported sample");
      assert.deepEqual(inputs(root), tested);
    } finally { fs.writeFileSync(test, originalTest); }
    assert.deepEqual(inputs(root), before);
    return first;
}

async function main() {
  const args = process.argv.slice(2);
  assert.equal(args.length, 6, "usage: --repo RIPGREP --binary HARDGATE --output FRESH_DIRECTORY");
  const options = Object.fromEntries([0, 2, 4].map((index) => [args[index], args[index + 1]]));
  assert.deepEqual(Object.keys(options).sort(), ["--binary", "--output", "--repo"]);
  const repo = path.resolve(options["--repo"]), output = path.resolve(options["--output"]), binary = path.resolve(options["--binary"]);
  fs.mkdirSync(output); // Never replace previous evidence.
  const original = inputs(repo);
  const revision = (await runReleaseProcess("git", ["rev-parse", "HEAD"], { cwd: repo, timeoutMs: 30_000 })).trim();
  fs.writeFileSync(path.join(output, "revision.txt"), `${revision}\n`);
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-ripgrep-acceptance-"));
  try {
    const root = path.join(temporary, "ripgrep");
    fs.cpSync(repo, root, { recursive: true, verbatimSymlinks: true, filter: (file) => !OMITTED.has(path.basename(file)) });
    const test = path.join(root, "crates/matcher/tests/test_matcher.rs");
    assert(fs.statSync(test).isFile());
    fs.writeFileSync(path.join(root, "hardgate.toml"), '[gate]\npreset = "balanced"\n');
    const cargoConfig = path.join(root, ".cargo");
    fs.mkdirSync(cargoConfig, { recursive: true });
    // Retain Cargo's normal lint enforcement and reuse the prerequisite build.
    // The runner's default lint cap otherwise creates another workspace build.
    fs.writeFileSync(path.join(cargoConfig, "mutants.toml"), "cap_lints = false\n", { flag: "wx" });
    const first = await exercise({ root, output, binary }, test);
    assert.deepEqual(inputs(repo), original);
    fs.writeFileSync(path.join(output, "acceptance.json"), JSON.stringify({ revision, ...first,
      survived: true, addedAssertionPassed: true, killedAfterAssertion: true, callerInputsRestored: true,
      originalCheckoutUnchanged: true, testDebugInfo: process.env.CARGO_PROFILE_TEST_DEBUG ?? "default",
    }, null, 2) + "\n");
    console.log("ByteSet acceptance passed. Selected mutation evidence does not establish repository-wide mutation coverage.");
  } finally { fs.rmSync(temporary, { recursive: true, force: true }); }
}

await main();
