// Behavioral contract for exact, resumable GitHub release staging.
"use strict";

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { performance } from "node:perf_hooks";
import { projectRoot } from "../scripts/release-support.mjs";
import { expectedGithubAssets, stageGithubRelease } from "../scripts/github-staging-state.mjs";
import "../scripts/stage-github-release.mjs";

const assets = ["one.tar.gz", "two.tar.gz", "SHA256SUMS"];
const request = (overrides = {}) => ({ tag: "v1.2.3", version: "1.2.3", assets, policy: { deadline: performance.now() + 10_000 }, ...overrides });

function scenario(probes, overrides = {}) {
  const events = [];
  const queue = [...probes];
  const operations = {
    probe: async () => {
      events.push("probe");
      const result = queue.shift();
      if (result instanceof Error) throw result;
      return result;
    },
    create: async () => { events.push("create"); return overrides.create?.(); },
    upload: async (_request, name) => { events.push(`upload:${name}`); return overrides.upload?.(name); },
    verify: async (_request, names) => { events.push(`verify:${names.join(",")}`); return overrides.verify?.(names); },
  };
  return { events, operations };
}

const present = (names = assets, prerelease = true) => ({ state: "present", tag: "v1.2.3", isDraft: false, isPrerelease: prerelease, assets: names });
const reject = (action, pattern) => assert.rejects(action, pattern);

const created = scenario([{ state: "missing" }, present()]);
assert.deepEqual(await stageGithubRelease(request(), created.operations), { publication: "created", state: "immutable_verified", prerelease: true });
assert.deepEqual(created.events, ["probe", "create", "probe", "verify:one.tar.gz,two.tar.gz,SHA256SUMS"]);

const stable = scenario([present(assets, false)]);
assert.deepEqual(await stageGithubRelease(request(), stable.operations), { publication: "existing", state: "immutable_verified", prerelease: false });
assert.deepEqual(stable.events, ["probe", "verify:one.tar.gz,two.tar.gz,SHA256SUMS"]);

const missingExisting = scenario([present([assets[0], assets[2]], true), present(assets, true), present(assets, true)]);
assert.deepEqual(await stageGithubRelease(request(), missingExisting.operations), { publication: "existing", state: "immutable_verified", prerelease: true });
assert.deepEqual(missingExisting.events, ["probe", "verify:one.tar.gz,SHA256SUMS", "upload:two.tar.gz", "probe", "verify:two.tar.gz", "probe", "verify:one.tar.gz,two.tar.gz,SHA256SUMS"]);

const uploadAmbiguous = scenario([present([assets[0]], false), present(assets, false), present(assets, false), present(assets, false)], { upload: () => { throw new Error("upload response lost"); } });
assert.equal((await stageGithubRelease(request(), uploadAmbiguous.operations)).publication, "ambiguous");
assert.equal(uploadAmbiguous.events.filter((event) => event === "upload:two.tar.gz").length, 1);

const createAmbiguous = scenario([{ state: "missing" }, present(assets, true)], { create: () => { throw new Error("create response lost"); } });
assert.equal((await stageGithubRelease(request(), createAmbiguous.operations)).publication, "ambiguous");
assert.equal(createAmbiguous.events.filter((event) => event === "create").length, 1);

const partialCreate = scenario([{ state: "missing" }, present([assets[0]], true)], { create: () => { throw new Error("create response lost"); } });
await reject(stageGithubRelease(request(), partialCreate.operations), /exact expected asset set/);
assert.equal(partialCreate.events.filter((event) => event === "create").length, 1);

const uploadUnknown = scenario([present([assets[0]], true), new Error("authorization denied")], { upload: () => { throw new Error("upload response lost"); } });
await reject(stageGithubRelease(request(), uploadUnknown.operations), /authorization denied/);
assert.equal(uploadUnknown.events.filter((event) => event === "upload:two.tar.gz").length, 1);

const mismatchBeforeUpload = scenario([present([assets[0]], true)], { verify: (names) => names.length === 1 ? (() => { throw new Error("existing bytes mismatch"); })() : undefined });
await reject(stageGithubRelease(request(), mismatchBeforeUpload.operations), /existing bytes mismatch/);
assert.equal(mismatchBeforeUpload.events.some((event) => event.startsWith("upload:")), false);

for (const bad of [present(["unexpected.tgz"], true), { ...present([assets[0]], true), tag: "v9.9.9" }, { ...present([assets[0]], true), isDraft: true }]) {
  const invalid = scenario([bad]);
  await reject(stageGithubRelease(request(), invalid.operations), /unexpected assets|tag mismatch|draft/);
  assert.equal(invalid.events.some((event) => event.startsWith("upload:") || event === "create"), false);
}

const authProbe = scenario([new Error("HTTP 401 Unauthorized")]);
await reject(stageGithubRelease(request(), authProbe.operations), /HTTP 401/);
assert.equal(authProbe.events.includes("create"), false);
const persistentMissing = scenario([{ state: "missing" }, new Error("release probe unavailable")]);
await reject(stageGithubRelease(request(), persistentMissing.operations), /release probe unavailable/);
assert.equal(persistentMissing.events.filter((event) => event === "create").length, 1);

const fixture = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-github-staging-test-"));
try {
  const dist = path.join(fixture, "dist");
  const bin = path.join(fixture, "bin");
  const remote = path.join(fixture, "remote");
  const log = path.join(fixture, "calls.jsonl");
  fs.mkdirSync(dist); fs.mkdirSync(bin); fs.mkdirSync(remote);
  const cliAssets = expectedGithubAssets("1.2.3");
  for (const [index, name] of cliAssets.entries()) fs.writeFileSync(path.join(dist, name), `fixture-${index}\n`);
  const fakeGh = `#!/usr/bin/env node
const fs=require('node:fs'); const path=require('node:path');
const args=process.argv.slice(2); const remote=${JSON.stringify(remote)}; const log=${JSON.stringify(log)};
fs.appendFileSync(log, JSON.stringify({args,env:Object.fromEntries(['GH_TOKEN','GITHUB_TOKEN','NPM_TOKEN','CARGO_REGISTRY_TOKEN','ACTIONS_ID_TOKEN_REQUEST_TOKEN'].map(k=>[k,process.env[k]??null]))})+'\\n');
const fail=(text)=>{process.stderr.write(text+'\\n');process.exit(1);}; const view=()=>{if(!fs.existsSync(path.join(remote,'created'))) fail('release not found'); const names=fs.readdirSync(remote).filter(n=>n!=='created'); process.stdout.write(JSON.stringify({tagName:'v1.2.3',isDraft:false,isPrerelease:true,assets:names.map(name=>({name}))}));};
if(args[0]!=='release') fail('unexpected command'); if(args[1]==='view') view();
else if(args[1]==='create'){const files=args.slice(3).filter(p=>fs.existsSync(p)); for(const file of files) fs.copyFileSync(file,path.join(remote,path.basename(file))); fs.writeFileSync(path.join(remote,'created'),'yes');}
else if(args[1]==='upload'){const file=args[3]; fs.copyFileSync(file,path.join(remote,path.basename(file)));}
else if(args[1]==='download'){const name=args[args.indexOf('--pattern')+1]; const directory=args[args.indexOf('--dir')+1]; fs.copyFileSync(path.join(remote,name),path.join(directory,name));}
else fail('unexpected release operation');
`;
  fs.writeFileSync(path.join(bin, "gh"), fakeGh, { mode: 0o755 });
  const runCli = (extra = []) => spawnSync(process.execPath, [path.join(projectRoot, "scripts/stage-github-release.mjs"), "--repo", "owner/repo", "--tag", "v1.2.3", "--version", "1.2.3", "--dist", dist, ...extra], {
    cwd: projectRoot, encoding: "utf8", env: { ...process.env, PATH: `${bin}${path.delimiter}${process.env.PATH}`, GH_TOKEN: "fixture-token", GITHUB_TOKEN: "must-not-leak", NPM_TOKEN: "must-not-leak", CARGO_REGISTRY_TOKEN: "must-not-leak", ACTIONS_ID_TOKEN_REQUEST_TOKEN: "must-not-leak" },
  });
  const cli = runCli();
  assert.equal(cli.status, 0, cli.stderr);
  assert.match(cli.stdout, /created .*immutable_verified.*prerelease=true/);
  const calls = fs.readFileSync(log, "utf8").trim().split("\n").map(JSON.parse);
  const createCall = calls.find((call) => call.args[1] === "create");
  assert.ok(createCall);
  for (const flag of ["--verify-tag", "--generate-notes", "--prerelease", "--latest=false"]) assert.ok(createCall.args.includes(flag), flag);
  assert.equal(createCall.args.includes("--clobber"), false);
  assert.equal(createCall.env.GH_TOKEN, "fixture-token");
  for (const key of ["GITHUB_TOKEN", "NPM_TOKEN", "CARGO_REGISTRY_TOKEN", "ACTIONS_ID_TOKEN_REQUEST_TOKEN"]) assert.equal(createCall.env[key], null, key);

  fs.writeFileSync(path.join(dist, "extra.txt"), "extra");
  const before = fs.readFileSync(log, "utf8");
  assert.notEqual(runCli().status, 0);
  assert.equal(fs.readFileSync(log, "utf8"), before);
  fs.rmSync(path.join(dist, "extra.txt"));
  fs.symlinkSync(path.join(dist, cliAssets[0]), path.join(dist, "symlink-target"));
  assert.notEqual(runCli().status, 0);
} finally {
  fs.rmSync(fixture, { recursive: true, force: true });
}

console.log("github_staging.test: OK (exact assets, create/upload reconciliation, byte proof, CLI isolation)");
