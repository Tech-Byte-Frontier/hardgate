#!/usr/bin/env node
// Emit a deterministic CycloneDX inventory for the Cargo dependency graph.
// Usage: node scripts/release-sbom.mjs --output dist/hardgate.sbom.cdx.json
"use strict";

import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";

function option(name, fallback) {
  const index = process.argv.indexOf(name);
  if (index >= 0) return process.argv[index + 1];
  return process.argv.find((value) => value.startsWith(`${name}=`))?.slice(name.length + 1) ?? fallback;
}

function cargoMetadata() {
  const result = spawnSync("cargo", ["metadata", "--locked", "--format-version", "1"], { encoding: "utf8" });
  if (result.error || result.status !== 0) {
    throw new Error(`cargo metadata failed: ${result.error?.message ?? result.stderr}`);
  }
  return JSON.parse(result.stdout);
}

function component(pkg) {
  const value = {
    type: "library",
    "bom-ref": `pkg:cargo/${pkg.name}@${pkg.version}`,
    name: pkg.name,
    version: pkg.version,
    purl: `pkg:cargo/${pkg.name}@${pkg.version}`,
  };
  if (pkg.license) value.licenses = [{ license: { id: pkg.license } }];
  return value;
}

const output = path.resolve(option("--output", "dist/hardgate.sbom.cdx.json"));
const metadata = cargoMetadata();
const components = metadata.packages.map(component).sort((left, right) => left.purl.localeCompare(right.purl));
const bom = {
  bomFormat: "CycloneDX",
  specVersion: "1.5",
  version: 1,
  metadata: { component: components.find((item) => item.name === "hardgate") },
  components,
};
fs.mkdirSync(path.dirname(output), { recursive: true });
fs.writeFileSync(output, `${JSON.stringify(bom, null, 2)}\n`);
console.log(`release-sbom: ${components.length} Cargo components -> ${output}`);
