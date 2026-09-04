// Runtime contract for the deterministic CycloneDX release inventory.
"use strict";

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { uuidV5 } from "../scripts/release-support.mjs";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const generator = path.join(root, "scripts/release-sbom.mjs");
const verifier = path.join(root, "scripts/release-sbom-verify.mjs");
const fixture = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-sbom-contract-"));
const firstPath = path.join(fixture, "first.cdx.json");
const secondPath = path.join(fixture, "second.cdx.json");

assert.equal(uuidV5("https://www.widgets.com"), "42343567-6fc3-5a6a-80a2-83d9a01cadaa", "UUIDv5 must match the RFC URL-namespace algorithm");

function run(script, args) {
  return spawnSync(process.execPath, [script, ...args], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 4 * 1024 * 1024,
  });
}

try {
  const first = run(generator, ["--output", firstPath]);
  assert.equal(first.status, 0, `first SBOM generation failed: ${first.stderr}`);
  const second = run(generator, ["--output", secondPath]);
  assert.equal(second.status, 0, `second SBOM generation failed: ${second.stderr}`);

  const firstBytes = fs.readFileSync(firstPath);
  const secondBytes = fs.readFileSync(secondPath);
  assert.deepEqual(secondBytes, firstBytes, "SBOM output must be byte-for-byte deterministic");

  const bom = JSON.parse(firstBytes.toString("utf8"));
  assert.match(
    bom.serialNumber,
    /^urn:uuid:[0-9a-f]{8}-[0-9a-f]{4}-5[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    "CycloneDX serialNumber must be a lowercase RFC 4122 UUIDv5 URN",
  );
  const verified = run(verifier, ["--file", firstPath, "--version", bom.metadata.component.version]);
  assert.equal(verified.status, 0, `SBOM verification failed: ${verified.stderr}`);

  const missingSerialPath = path.join(fixture, "missing-serial.cdx.json");
  const missingSerial = { ...bom };
  delete missingSerial.serialNumber;
  fs.writeFileSync(missingSerialPath, `${JSON.stringify(missingSerial)}\n`);
  const missingSerialResult = run(verifier, ["--file", missingSerialPath]);
  assert.notEqual(missingSerialResult.status, 0, "verifier must reject a CycloneDX document without serialNumber");
  assert.match(missingSerialResult.stderr, /serialNumber must be a lowercase RFC 4122 UUIDv5 URN/);

  const changedBodyPath = path.join(fixture, "changed-body.cdx.json");
  const changedBody = structuredClone(bom);
  changedBody.metadata.component.version = "0.0.0-tampered";
  fs.writeFileSync(changedBodyPath, `${JSON.stringify(changedBody)}\n`);
  const changedBodyResult = run(verifier, ["--file", changedBodyPath]);
  assert.notEqual(changedBodyResult.status, 0, "verifier must reject a serialNumber copied onto different BOM contents");
  assert.match(changedBodyResult.stderr, /serialNumber must deterministically identify/);

  console.log("release_contract.sbom: deterministic GitHub-attestable CycloneDX inventory");
} finally {
  fs.rmSync(fixture, { recursive: true, force: true });
}
