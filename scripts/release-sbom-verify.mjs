#!/usr/bin/env node
// Validate the structural CycloneDX contract emitted by release-sbom.mjs.
// Usage: node scripts/release-sbom-verify.mjs --file dist/hardgate.sbom.cdx.json --version <version>
"use strict";

import fs from "node:fs";
import path from "node:path";
import { option } from "./release-support.mjs";

function fail(message) {
  throw new Error(`release-sbom-verify: ${message}`);
}

const file = path.resolve(option("--file", "dist/hardgate.sbom.cdx.json"));
const expectedVersion = option("--version");
if (!fs.existsSync(file)) fail(`missing ${file}`);
let bom;
try {
  bom = JSON.parse(fs.readFileSync(file, "utf8"));
} catch (error) {
  fail(`invalid JSON: ${error.message}`);
}
if (bom["$schema"] !== "http://cyclonedx.org/schema/bom-1.5.schema.json" || bom.bomFormat !== "CycloneDX" || bom.specVersion !== "1.5" || bom.version !== 1) {
  fail("must be CycloneDX 1.5 version 1");
}
const root = bom.metadata?.component;
if (!root || root.type !== "application" || root.name !== "hardgate" || typeof root.version !== "string" || typeof root["bom-ref"] !== "string") {
  fail("metadata.component must identify hardgate with a bom-ref and version");
}
if (expectedVersion && root.version !== expectedVersion) {
  fail(`metadata.component version ${root.version} does not match ${expectedVersion}`);
}
if (!Array.isArray(bom.components) || bom.components.length === 0) fail("components must be a non-empty array");
function validateLicenseId(component, license) {
  if (typeof license.id !== "string" || !/^[A-Za-z0-9.-]+$/.test(license.id)) {
    fail(`${component.name} license.id must be a single SPDX identifier`);
  }
  if (Object.hasOwn(license, "expression")) fail(`${component.name} license cannot contain id and expression`);
}

function validateLicenseExpression(component, license) {
  if (typeof license.expression !== "string" || !/^[A-Za-z0-9.+()\- ]+$/.test(license.expression) || !/\b(?:AND|OR|WITH)\b/.test(license.expression)) {
    fail(`${component.name} license.expression is not a valid SPDX expression shape`);
  }
}

function validateLicenseEntry(component, entry) {
  const license = entry?.license;
  if (!license || typeof license !== "object") fail(`${component.name} has malformed license entry`);
  if (Object.hasOwn(license, "id")) return validateLicenseId(component, license);
  if (Object.hasOwn(license, "expression")) return validateLicenseExpression(component, license);
  fail(`${component.name} license must contain id or expression`);
}

function validateLicenses(component) {
  for (const entry of component.licenses ?? []) validateLicenseEntry(component, entry);
}
validateLicenses(root);
const refs = new Set([root["bom-ref"]]);
for (const component of bom.components) {
  if (!component || component.type !== "library" || typeof component.name !== "string" || typeof component.version !== "string") {
    fail("every dependency component must be a typed library with name/version");
  }
  if (component["bom-ref"] === root["bom-ref"]) {
    fail("metadata.component must not be duplicated as a dependency component");
  }
  if (typeof component["bom-ref"] !== "string" || refs.has(component["bom-ref"])) {
    fail("all bom-ref values must be unique strings");
  }
  refs.add(component["bom-ref"]);
  validateLicenses(component);
}
console.log(`release-sbom-verify: ${bom.components.length + 1} CycloneDX components structurally valid`);
