#!/usr/bin/env node
// Sync npm/* versions from Cargo.toml [package] version (single source of truth).
// Usage: node scripts/sync-npm-version.mjs [--check]
"use strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const cargoToml = fs.readFileSync(path.join(root, "Cargo.toml"), "utf8");
const m = cargoToml.match(/^version\s*=\s*"([^"]+)"/m);
if (!m) {
  console.error("sync-npm-version: could not parse version from Cargo.toml");
  process.exit(1);
}
const version = m[1];
const check = process.argv.includes("--check");

const platformPkgs = [
  "hardgate-linux-x64",
  "hardgate-linux-x64-musl",
  "hardgate-linux-arm64",
  "hardgate-linux-arm64-musl",
  "hardgate-darwin-x64",
  "hardgate-darwin-arm64",
  "hardgate-win32-x64",
];

let dirty = false;
function syncJson(file, mutate) {
  const before = fs.readFileSync(file, "utf8");
  const json = JSON.parse(before);
  mutate(json);
  const after = JSON.stringify(json, null, 2) + "\n";
  if (after !== before) {
    dirty = true;
    console.log(`sync: ${path.relative(root, file)} -> ${version}`);
    if (!check) fs.writeFileSync(file, after);
  }
}

syncJson(path.join(root, "npm/hardgate/package.json"), (j) => {
  j.version = version;
  j.optionalDependencies ??= {};
  for (const p of platformPkgs) j.optionalDependencies[p] = version;
});
for (const p of platformPkgs) {
  syncJson(path.join(root, `npm/${p}/package.json`), (j) => {
    j.version = version;
  });
}

if (check && dirty) {
  console.error(
    "sync-npm-version --check: npm/* out of sync with Cargo.toml. Run `node scripts/sync-npm-version.mjs`.",
  );
  process.exit(1);
}
console.log(`versions synced: ${version}`);
