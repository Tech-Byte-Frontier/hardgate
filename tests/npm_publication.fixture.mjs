// Fresh local archives and fake registry executables: no registry writes.
"use strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";

const fakeNpm = `
import fs from 'node:fs';
import path from 'node:path';
const state = process.env.FIXTURE_STATE;
const count = Number(fs.existsSync(state) ? fs.readFileSync(state, 'utf8') : 0) + 1;
fs.writeFileSync(state, String(count));
if (process.argv[2] !== 'pack') throw new Error('only pack is permitted');
if (!process.argv.includes('--ignore-scripts') || !process.argv.includes('--loglevel=error') || process.argv.includes('--silent')) throw new Error('npm verification must disable scripts and preserve error diagnostics');
const mode = process.env.FIXTURE_MODE;
if (mode === 'stall') {
  process.on('SIGTERM', () => {});
  fs.writeFileSync(state + '.pid', String(process.pid));
  setInterval(() => {}, 1000);
} else if (mode !== 'success' && (count === 1 || mode.endsWith('-forever'))) {
  console.error('npm error code ' + mode.replace('-forever', ''));
  if (mode === 'E429') console.error('Retry-After: 0');
  process.exitCode = 1;
} else {
  const directory = process.argv[process.argv.indexOf('--pack-destination') + 1];
  fs.copyFileSync(process.env.FIXTURE_ARCHIVE, path.join(directory, 'fixture.tgz'));
}
`;
const fakeCurl = `
const mode = process.env.FIXTURE_METADATA;
if (mode === 'missing') console.log('{}\\n404');
else if (mode === 'auth') console.log('{}\\n403');
else if (mode === 'malformed') console.log('invalid json\\n200');
else console.log(JSON.stringify({name: mode === 'wrong' ? 'unexpected' : 'hardgate-linux-x64', version: '0.5.0'}) + '\\n200');
`;

function archive(directory, output, member) {
  const result = spawnSync("tar", ["-czf", output, "-C", directory, member], { encoding: "utf8" });
  if (result.status !== 0) throw new Error(result.stderr);
}

function writeExecutable(filename, content) {
  fs.writeFileSync(filename, content, { mode: 0o755 });
}

export function publicationFixture() {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-publication-test-"));
  for (const name of ["bin", "dist", "release/hardgate-linux-x64", "package/bin"]) fs.mkdirSync(path.join(directory, name), { recursive: true });
  writeExecutable(path.join(directory, "bin/npm"), `#!${process.execPath}\n${fakeNpm}`);
  writeExecutable(path.join(directory, "bin/curl"), `#!${process.execPath}\n${fakeCurl}`);
  writeExecutable(path.join(directory, "release/hardgate-linux-x64/hardgate"), "verified release bytes");
  archive(path.join(directory, "release"), path.join(directory, "dist/hardgate-linux-x64.tar.gz"), "hardgate-linux-x64");
  return { directory, state: path.join(directory, "attempts"), archive: path.join(directory, "package.tgz") };
}

export function stagePackage(fixture, overrides = {}) {
  const manifest = { name: "hardgate-linux-x64", version: "0.5.0", os: ["linux"], cpu: ["x64"], libc: ["glibc"], ...overrides.manifest };
  fs.writeFileSync(path.join(fixture.directory, "package/package.json"), JSON.stringify(manifest));
  const binary = path.join(fixture.directory, "package/bin/hardgate");
  fs.writeFileSync(binary, overrides.binary ?? "verified release bytes");
  fs.chmodSync(binary, overrides.mode ?? 0o755);
  archive(fixture.directory, fixture.archive, "package");
  fs.rmSync(fixture.state, { force: true });
}

export function fixtureEnvironment(fixture, overrides = {}) {
  return {
    ...process.env, PATH: `${path.join(fixture.directory, "bin")}${path.delimiter}${process.env.PATH}`,
    FIXTURE_STATE: fixture.state, FIXTURE_ARCHIVE: fixture.archive, FIXTURE_MODE: "success", FIXTURE_METADATA: "valid",
    NPM_VERIFY_ATTEMPTS: "3", NPM_VERIFY_DELAY_SECONDS: "0", NPM_VERIFY_TIMEOUT_SECONDS: "10", NPM_VERIFY_CHILD_TIMEOUT_SECONDS: "2", ...overrides,
  };
}
