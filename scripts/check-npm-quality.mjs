#!/usr/bin/env node
// Quality gate for npm/* packages (registry standards).
// Usage: node scripts/check-npm-quality.mjs
// Checks: metadata (license/author/publishConfig/type/repo/bugs),
// files[] includes README + both licenses, platform READMEs exist,
// versions + optionalDependencies in sync with Cargo.toml.
"use strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const cargoToml = fs.readFileSync(path.join(root, "Cargo.toml"), "utf8");
const version = cargoToml.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
if (!version) {
  console.error("check-npm-quality: could not parse version from Cargo.toml");
  process.exit(1);
}

const platformPkgs = [
  "hardgate-linux-x64",
  "hardgate-linux-x64-musl",
  "hardgate-linux-arm64",
  "hardgate-linux-arm64-musl",
  "hardgate-darwin-x64",
  "hardgate-darwin-arm64",
  "hardgate-win32-x64",
];

let failures = 0;
const fail = (msg) => {
  failures += 1;
  console.error(`FAIL: ${msg}`);
};

const readJson = (rel) =>
  JSON.parse(fs.readFileSync(path.join(root, rel), "utf8"));

// --- main wrapper ---
const main = readJson("npm/hardgate/package.json");
if (main.version !== version) fail(`npm/hardgate version ${main.version} != Cargo ${version}`);
for (const p of platformPkgs) {
  if (main.optionalDependencies?.[p] !== version)
    fail(`npm/hardgate optionalDependencies[${p}] != ${version}`);
}
for (const f of ["bin/", "README.md", "LICENSE-MIT", "LICENSE-APACHE"]) {
  if (!main.files?.includes(f)) fail(`npm/hardgate files[] missing ${f}`);
}
if (main.license !== "(MIT OR Apache-2.0)") fail(`npm/hardgate license = ${main.license}`);
if (!main.author) fail("npm/hardgate missing author");
if (main.publishConfig?.access !== "public") fail("npm/hardgate missing publishConfig.access=public");
if (!main.repository?.url?.startsWith("git+https://")) fail("npm/hardgate repository.url should be git+https://");
if (!fs.existsSync(path.join(root, "npm/hardgate/README.md"))) fail("npm/hardgate/README.md missing");
if (!fs.existsSync(path.join(root, "npm/hardgate/bin/hardgate.js"))) fail("npm/hardgate/bin/hardgate.js missing");

// --- platform packages ---
for (const p of platformPkgs) {
  const j = readJson(`npm/${p}/package.json`);
  if (j.version !== version) fail(`npm/${p} version ${j.version} != Cargo ${version}`);
  if (j.license !== "(MIT OR Apache-2.0)") fail(`npm/${p} license = ${j.license}`);
  if (!j.author) fail(`npm/${p} missing author`);
  if (j.publishConfig?.access !== "public") fail(`npm/${p} missing publishConfig.access=public`);
  if (!j.repository?.url?.startsWith("git+https://")) fail(`npm/${p} repository.url should be git+https://`);
  if (!j.bugs) fail(`npm/${p} missing bugs`);
  for (const f of ["bin/", "README.md", "LICENSE-MIT", "LICENSE-APACHE"]) {
    if (!j.files?.includes(f)) fail(`npm/${p} files[] missing ${f}`);
  }
  if (!fs.existsSync(path.join(root, `npm/${p}/README.md`))) fail(`npm/${p}/README.md missing`);
  if (!j.os?.length || !j.cpu?.length) fail(`npm/${p} missing os/cpu`);
}

if (failures) {
  console.error(`check-npm-quality: ${failures} failure(s)`);
  process.exit(1);
}
console.log(`check-npm-quality: ok (${platformPkgs.length} platform pkgs + wrapper @ ${version})`);
