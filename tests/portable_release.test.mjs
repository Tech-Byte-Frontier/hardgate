// Every native platform must supply identity-bound consumer proof, including
// successful jobs retained from an earlier partial workflow attempt.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { PLATFORM_NAMES, PLATFORM_ASSETS, executableName } from "../scripts/release-platforms.mjs";
import { createReceipt, recordTransition, readReceipt, writeReceiptAtomicSync } from "../scripts/release-receipt.mjs";

const root = fileURLToPath(new URL("../", import.meta.url));
const directory = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-portable-proof-"));
const receiptPath = path.join(directory, "receipt.json");
const names = PLATFORM_NAMES.filter((name) => name !== "hardgate-linux-x64");
const identity = {
  version: "1.2.3", source_sha: "a".repeat(40), tooling_sha: "b".repeat(40),
  signed_tag_object: "c".repeat(40), build_run_id: "123", artifact_id: "456",
  archives: [...PLATFORM_ASSETS].sort().map((name) => ({ name, sha256: "d".repeat(64) })),
};
const receipt = createReceipt(identity);
for (const name of names) {
  for (const [from, to] of [["pending", "staged"], ["staged", "immutable_verified"]]) {
    recordTransition(receipt, { channel: name, from, to, evidence: {
      version: identity.version, source_sha: identity.source_sha, archives: identity.archives,
    } });
  }
}
function proof(name, attempt = 1, source = identity.source_sha) {
  const folder = path.join(directory, `portable-exact-${name}-attempt-${attempt}`);
  fs.mkdirSync(folder, { recursive: true });
  const value = {
    schema_version: 1, version: identity.version, source_sha: source, mode: "exact", package: name,
    archive: identity.archives.find((entry) => entry.name === `${name}.tar.gz`),
    consumer: { executable: `node_modules/${name}/bin/${executableName(name)}`, sha256: "e".repeat(64) },
  };
  fs.writeFileSync(path.join(folder, `${name}.json`), JSON.stringify(value));
}
function apply() {
  return spawnSync(process.execPath, [path.join(root, "scripts/apply-portable-proofs.mjs"), receiptPath, directory, "exact"], { encoding: "utf8" });
}
try {
  writeReceiptAtomicSync(receiptPath, receipt);
  for (const name of names.slice(0, -1)) proof(name);
  const before = fs.readFileSync(receiptPath, "utf8");
  assert.notEqual(apply().status, 0, "a missing platform cannot advance the receipt");
  assert.equal(fs.readFileSync(receiptPath, "utf8"), before);
  proof(names.at(-1));
  proof(names[0], 2, "f".repeat(40));
  assert.notEqual(apply().status, 0, "a newer proof with a different source cannot fall back to an older success");
  assert.equal(fs.readFileSync(receiptPath, "utf8"), before);
  proof(names[0], 2);
  const result = apply();
  assert.equal(result.status, 0, result.stderr);
  const completed = readReceipt(receiptPath);
  for (const name of names) assert.equal(completed.channels[name].state, "exact_consumer_verified");
  assert.equal(completed.channels["hardgate-linux-x64"].state, "pending");
  for (const name of names) assert.equal(completed.channels[name].events.at(-1).evidence.consumer.executable,
    `node_modules/${name}/bin/hardgate`);
  console.log("portable_release: missing, mismatched, and mixed-attempt consumer proofs verified");
} finally {
  fs.rmSync(directory, { recursive: true, force: true });
}
