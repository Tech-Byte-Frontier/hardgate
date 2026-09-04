#!/usr/bin/env node
// Emit the deterministic SHA256SUMS manifest for every release payload.
// Usage: node scripts/release-checksums.mjs --dist dist --version <version>
"use strict";

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { option } from "./release-support.mjs";

const packages = [
  "hardgate-linux-x64",
  "hardgate-linux-x64-musl",
  "hardgate-linux-arm64",
  "hardgate-linux-arm64-musl",
  "hardgate-darwin-x64",
  "hardgate-darwin-arm64",
];

function fail(message) {
  throw new Error(`release-checksums: ${message}`);
}

const dist = path.resolve(option("--dist", "dist"));
const version = option("--version");
if (!version) fail("--version is required");
const names = [
  ...packages.map((name) => `${name}.tar.gz`),
  `hardgate-${version}.sbom.cdx.json`,
];
const lines = names.map((name) => {
  const file = path.join(dist, name);
  if (!fs.existsSync(file)) fail(`missing release payload ${name}`);
  const digest = crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
  return `${digest}  ${name}`;
});
fs.writeFileSync(path.join(dist, "SHA256SUMS"), `${lines.join("\n")}\n`);
console.log(`release-checksums: ${lines.length} payloads covered by SHA256SUMS`);
