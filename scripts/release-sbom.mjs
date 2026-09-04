#!/usr/bin/env node
// Emit a deterministic CycloneDX inventory for the Cargo dependency graph.
// Usage: node scripts/release-sbom.mjs --output dist/hardgate.sbom.cdx.json
"use strict";

import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { option, uuidV5 } from "./release-support.mjs";

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
  if (pkg.license) {
    // A few older Cargo manifests use the legacy `Unlicense/MIT` spelling;
    // CycloneDX carries SPDX expressions, so normalize that separator before
    // classifying the value.
    const licenseText = pkg.license.trim().replace(/\s*\/\s*/g, " OR ");
    // CycloneDX distinguishes a single SPDX identifier from a compound SPDX
    // expression. Never put an expression such as `MIT OR Apache-2.0` in the
    // `license.id` field: consumers validate that field as one identifier.
    const license = /^[A-Za-z0-9.-]+$/.test(licenseText)
      ? { id: licenseText }
      : { expression: licenseText };
    value.licenses = [{ license }];
  }
  return value;
}

function codepointCompare(left, right) {
  // Compare UTF-8 bytes explicitly so ordering is independent of locale and
  // JavaScript UTF-16 collation details across runners.
  return Buffer.from(left, "utf8").compare(Buffer.from(right, "utf8"));
}

const output = path.resolve(option("--output", "dist/hardgate.sbom.cdx.json"));
const metadata = cargoMetadata();
const components = metadata.packages.map(component).sort((left, right) => codepointCompare(left.purl, right.purl));
const rootComponent = components.find((item) => item.name === "hardgate");
if (!rootComponent) throw new Error("release-sbom: Cargo metadata has no hardgate root component");
rootComponent.type = "application";
const bom = {
  "$schema": "http://cyclonedx.org/schema/bom-1.5.schema.json",
  bomFormat: "CycloneDX",
  specVersion: "1.5",
  version: 1,
  metadata: { component: rootComponent },
  // CycloneDX represents the application in metadata.component; dependency
  // components must not repeat that same bom-ref at the top level.
  components: components.filter((item) => item["bom-ref"] !== rootComponent["bom-ref"]),
};
// GitHub's SBOM attestation parser requires the CycloneDX serial number.
// UUIDv5 over the stable inventory makes changed BOM contents distinct while
// retaining the release pipeline's byte-for-byte reproducibility contract.
bom.serialNumber = `urn:uuid:${uuidV5(JSON.stringify(bom))}`;
fs.mkdirSync(path.dirname(output), { recursive: true });
fs.writeFileSync(output, `${JSON.stringify(bom, null, 2)}\n`);
console.log(`release-sbom: ${components.length} Cargo components -> ${output}`);
