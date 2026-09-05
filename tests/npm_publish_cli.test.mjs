// The real publisher entrypoint sees fake registry/npm commands only.
"use strict";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { projectRoot } from "../scripts/release-support.mjs";
import { fixtureEnvironment, publicationFixture, stagePackage } from "./npm_publication.fixture.mjs";

const npm = `
import fs from 'node:fs'; import path from 'node:path';
const state = process.env.FIXTURE_STATE;
if (process.argv[2] === 'publish') {
  if (process.env.FIXTURE_AUTH_MODE !== 'trusted' && process.env.NODE_AUTH_TOKEN !== 'fixture-auth') throw new Error('publish credential missing');
  if (!process.env.ACTIONS_ID_TOKEN_REQUEST_TOKEN || !process.env.ACTIONS_ID_TOKEN_REQUEST_URL) throw new Error('provenance credentials missing');
  if (process.env.FIXTURE_AUTH_MODE === 'trusted' && process.env.NODE_AUTH_TOKEN) throw new Error('trusted mode received token');
  if (process.argv[process.argv.indexOf('--tag') + 1] !== 'hardgate-candidate') throw new Error('default channel promoted during staging');
  if (!process.argv.includes('--provenance') || !process.argv.includes('--ignore-scripts')) throw new Error('publication protection missing');
  fs.appendFileSync(state + '.publishes', 'publish\\n');
  if (process.env.FIXTURE_MODE !== 'denied') fs.writeFileSync(state + '.visible', 'yes');
  if (process.env.FIXTURE_MODE !== 'success') {
    console.error(process.env.FIXTURE_MODE === 'denied' ? 'E403' : 'ETIMEDOUT');
    process.exitCode = 1;
  }
} else if (process.argv[2] === '--version') {
  console.log('12.0.2');
} else if (process.argv[2] === 'pack') {
  if (process.env.NODE_AUTH_TOKEN || process.env.ACTIONS_ID_TOKEN_REQUEST_TOKEN) throw new Error('credential leaked to archive retrieval');
  const directory = process.argv[process.argv.indexOf('--pack-destination') + 1];
  fs.copyFileSync(process.env.FIXTURE_ARCHIVE, path.join(directory, 'fixture.tgz'));
} else throw new Error('unexpected npm operation');
`;
const curl = `
import fs from 'node:fs';
if (process.env.NODE_AUTH_TOKEN || process.env.ACTIONS_ID_TOKEN_REQUEST_TOKEN) throw new Error('credential leaked to registry probe');
if (fs.existsSync(process.env.FIXTURE_STATE + '.visible')) console.log(JSON.stringify({name:'hardgate-linux-x64',version:'0.5.0'}) + '\\n200');
else console.log('{}\\n404');
`;

function runCase(options) {
  const fixture = publicationFixture();
  try {
    stagePackage(fixture, options.mismatch ? { binary: "mismatched bytes" } : {});
    fs.writeFileSync(path.join(fixture.directory, "bin/npm"), `#!${process.execPath}\n${npm}`);
    fs.writeFileSync(path.join(fixture.directory, "bin/curl"), `#!${process.execPath}\n${curl}`);
    if (options.existing) fs.writeFileSync(`${fixture.state}.visible`, "yes");
    const result = spawnSync(process.execPath, [path.join(projectRoot, "scripts/publish-npm-package.mjs"), "--version", "0.5.0", "--package-dir", path.join(fixture.directory, "package"), "--dist", path.join(fixture.directory, "dist")], {
      encoding: "utf8", timeout: 15000,
      env: fixtureEnvironment(fixture, { NODE_AUTH_TOKEN: "fixture-auth", GITHUB_ACTIONS: "true", ACTIONS_ID_TOKEN_REQUEST_TOKEN: "fixture-oidc", ACTIONS_ID_TOKEN_REQUEST_URL: "https://oidc.example.test/request", HARDGATE_NPM_AUTH_MODE: options.auth ?? "token", FIXTURE_AUTH_MODE: options.auth ?? "token", FIXTURE_MODE: options.mode ?? "success" }),
    });
    assert.equal(result.status === 0, options.success, result.stderr);
    const publishes = fs.existsSync(`${fixture.state}.publishes`) ? fs.readFileSync(`${fixture.state}.publishes`, "utf8").trim().split("\n").length : 0;
    assert.equal(publishes, options.existing ? 0 : 1);
    assert.doesNotMatch(result.stdout + result.stderr, /fixture-auth|fixture-oidc/);
    if (options.success) assert.match(result.stdout, /"state":"verified"/);
  } finally {
    fs.rmSync(fixture.directory, { recursive: true, force: true });
  }
}

for (const options of [
  { success: true }, { success: true, auth: "trusted" }, { success: true, existing: true }, { success: true, mode: "ambiguous" },
  { success: false, existing: true, mismatch: true }, { success: false, mismatch: true }, { success: false, mode: "denied" },
]) runCase(options);
console.log("npm_publish_cli.test: OK (real entrypoint, byte proof, publish once, credential scope)");
