// Exercise the actual shell download/checksum sequence with a local registry.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { PLATFORM_ASSETS } from "../scripts/release-platforms.mjs";

const script = fileURLToPath(new URL("../scripts/release-direct-consumer.sh", import.meta.url));
const root = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-direct-consumer-"));
const dist = path.join(root, "dist");
const registry = path.join(root, "registry");
const events = path.join(root, "events.jsonl");
const version = "1.2.3";
const commit = "a".repeat(40);
const assets = [...PLATFORM_ASSETS, `hardgate-${version}.sbom.cdx.json`];
const write = (name, bytes, mode = 0o644) => {
  const file = path.join(root, name);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, bytes, { mode });
};
const run = (phase = "exact") => {
  fs.writeFileSync(events, "");
  return spawnSync("bash", [script, phase], {
    cwd: root, encoding: "utf8", timeout: 30_000,
    env: { ...process.env, PATH: `${root}/bin${path.delimiter}${process.env.PATH}`,
      RELEASE_TAG: `v${version}`, RELEASE_VERSION: version, RELEASE_COMMIT: commit,
      REGISTRY_ROOT: registry, EVENT_LOG: events },
  });
};

try {
  write("content/hardgate-linux-x64/hardgate", `#!/bin/sh\nprintf '%s\\n' 'hardgate ${version} (${commit})'\n`, 0o755);
  fs.mkdirSync(dist);
  const packed = spawnSync("tar", ["-czf", `${dist}/hardgate-linux-x64.tar.gz`, "-C", `${root}/content`, "hardgate-linux-x64"], { encoding: "utf8" });
  assert.equal(packed.status, 0, packed.stderr);
  for (const asset of assets.filter(name => name !== "hardgate-linux-x64.tar.gz")) write(`dist/${asset}`, `fixture ${asset}\n`);
  write("dist/SHA256SUMS", assets.map(name => `${createHash("sha256").update(fs.readFileSync(`${dist}/${name}`)).digest("hex")}  ${name}\n`).join(""));
  fs.cpSync(dist, registry, { recursive: true });
  write("bin/gh", `#!${process.execPath}
const fs = require("node:fs");
const path = require("node:path");
const args = process.argv.slice(2);
if (args[0] !== "release") process.exit(2);
if (args[1] === "view") console.log(args.includes("tagName") ? process.env.RELEASE_TAG : "true");
else if (args[1] === "download") {
  const asset = args[args.indexOf("--pattern") + 1];
  fs.copyFileSync(path.join(process.env.REGISTRY_ROOT, asset), path.join(args[args.indexOf("--dir") + 1], asset));
} else process.exit(2);
`, 0o755);
  for (const name of ["installed-check.mjs", "release-receipt-cli.mjs"]) write(`release-tooling/scripts/${name}`,
    'import fs from "node:fs"; fs.appendFileSync(process.env.EVENT_LOG, JSON.stringify(process.argv.slice(1)) + "\\n");\n');

  for (const [phase, state] of [["exact", "exact_consumer_verified"], ["default", "default_consumer_verified"]]) {
    const result = run(phase);
    assert.equal(result.status, 0, result.stderr);
    for (const asset of assets) assert.ok(result.stdout.includes(`${asset}: OK`), `missing checksum verification: ${asset}`);
    const calls = fs.readFileSync(events, "utf8").trim().split("\n").map(line => JSON.parse(line));
    assert.equal(calls.length, 2);
    assert.ok(calls[0][0].endsWith("installed-check.mjs"));
    assert.deepEqual(calls[1].slice(1, -2), ["advance", "--receipt", "receipt/release.json", "--channel", "github-assets", "--to", state]);
  }

  const foreign = assets.find(name => name !== "hardgate-linux-x64.tar.gz");
  const original = fs.readFileSync(`${registry}/${foreign}`);
  fs.rmSync(`${registry}/${foreign}`);
  assert.notEqual(run().status, 0, "a missing foreign archive must block the complete bundle check");
  assert.equal(fs.readFileSync(events, "utf8"), "");
  fs.writeFileSync(`${registry}/${foreign}`, "different published bytes");
  assert.notEqual(run().status, 0, "published bytes must match the verified bundle");
  assert.equal(fs.readFileSync(events, "utf8"), "");
  fs.writeFileSync(`${registry}/${foreign}`, original);
  const checksums = fs.readFileSync(`${dist}/SHA256SUMS`, "utf8").replace(/^[a-f0-9]{64}/, "0".repeat(64));
  fs.writeFileSync(`${dist}/SHA256SUMS`, checksums);
  fs.writeFileSync(`${registry}/SHA256SUMS`, checksums);
  assert.notEqual(run().status, 0, "matching manifests must still pass strict checksum verification");
  assert.equal(fs.readFileSync(events, "utf8"), "");
  console.log("release_direct_consumer: complete bundle, missing assets, byte mismatch, and strict checksums verified");
} finally {
  fs.rmSync(root, { recursive: true, force: true });
}
