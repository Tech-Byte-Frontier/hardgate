#!/usr/bin/env node
// Sync npm/* versions from Cargo.toml [package] version (single source of truth).
// Usage: node scripts/sync-npm-version.mjs [--check] [--tag vX.Y.Z]
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
const cargoVersion = m[1];
const check = process.argv.includes("--check");
const tagIndex = process.argv.indexOf("--tag");
const tagArg = tagIndex >= 0
  ? process.argv[tagIndex + 1]
  : process.argv.find((value) => value.startsWith("--tag="))?.slice(6);
if (tagIndex >= 0 && !tagArg) {
  console.error("sync-npm-version: --tag requires a v<semver> value");
  process.exit(1);
}
const tagVersion = tagArg?.startsWith("v") ? tagArg.slice(1) : null;
if (tagArg && !tagVersion) {
  console.error("sync-npm-version: --tag must be v<semver>");
  process.exit(1);
}
if (tagVersion && tagVersion !== cargoVersion) {
  console.error(`sync-npm-version: tag ${tagArg} does not match Cargo.toml ${cargoVersion}`);
  process.exit(1);
}
const version = tagVersion ?? cargoVersion;

const platformPkgs = [
  "hardgate-linux-x64",
  "hardgate-linux-x64-musl",
  "hardgate-linux-arm64",
  "hardgate-linux-arm64-musl",
  "hardgate-darwin-x64",
  "hardgate-darwin-arm64",
];

// Canonical npm metadata. Single source of truth for versions is Cargo.toml;
// the fields below keep `npm/*` aligned with npm registry quality standards
// (readme + license files present, SPDX dual-license, public provenance).
// `files` entries may not exist in git (LICENSE/README are copied into
// `npm/<pkg>/` by the release workflow before `npm publish`); npm ignores
// missing `files` entries on pack, so listing them here is safe locally.
const META = {
  license: "(MIT OR Apache-2.0)",
  author: "Tauan BF <contact@techbytefrontier.com>",
  homepage: "https://github.com/Tech-Byte-Frontier/hardgate",
  repository: {
    type: "git",
    url: "git+https://github.com/Tech-Byte-Frontier/hardgate.git",
  },
  bugs: "https://github.com/Tech-Byte-Frontier/hardgate/issues",
  publishConfig: { access: "public", provenance: true },
  type: "commonjs",
};

function applyCommon(j) {
  j.license = META.license;
  j.author = META.author;
  j.homepage = META.homepage;
  j.repository = { ...META.repository };
  j.bugs = META.bugs;
  j.publishConfig = { ...META.publishConfig };
  j.type = META.type;
}

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

// Keep the private workspace manifest aligned too: release-version validates
// it as a source of truth, so --check must detect drift before a tag is cut.
syncJson(path.join(root, "package.json"), (j) => {
  j.version = version;
});
syncJson(path.join(root, "npm/hardgate/package.json"), (j) => {
  j.version = version;
  applyCommon(j);
  j.files = ["bin/", "README.md", "LICENSE-MIT", "LICENSE-APACHE"];
  j.optionalDependencies ??= {};
  for (const p of platformPkgs) j.optionalDependencies[p] = version;
  // Prune entries for dropped platforms so the manifest never advertises
  // packages that no longer exist.
  for (const key of Object.keys(j.optionalDependencies)) {
    if (key.startsWith("hardgate-") && !platformPkgs.includes(key)) {
      delete j.optionalDependencies[key];
    }
  }
});
for (const p of platformPkgs) {
  syncJson(path.join(root, `npm/${p}/package.json`), (j) => {
    j.version = version;
    applyCommon(j);
    j.files = ["bin/", "README.md", "LICENSE-MIT", "LICENSE-APACHE"];
  });
}

if (check && dirty) {
  console.error(
    "sync-npm-version --check: npm/* out of sync with Cargo.toml. Run `node scripts/sync-npm-version.mjs`.",
  );
  process.exit(1);
}
console.log(`versions synced: ${version}`);
