import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import { openWorkloadSlot, acquireWorkloadSlot } from "../scripts/resource-scope-lock.mjs";
import { acknowledge, launchArguments, scopeActive } from "../scripts/resource-scope.mjs";

const root = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-resource-scope-test-"));
const execute = (command, args, options) => new Promise((resolve, reject) => {
  const child = spawn(command, args, options);
  child.once("error", reject);
  child.once("exit", (code) => resolve(code));
});
try {
  const directory = path.join(root, "slot"), fd = openWorkloadSlot(directory);
  try {
    await acquireWorkloadSlot(fd, execute);
    const other = openWorkloadSlot(directory);
    try {
      assert.equal(await execute("flock", ["--exclusive", "--nonblock", "--conflict-exit-code", "75", "3"], { stdio: ["ignore", "ignore", "ignore", other] }), 75);
    } finally { fs.closeSync(other); }
  } finally { fs.closeSync(fd); }
  const reusable = openWorkloadSlot(directory);
  try { await acquireWorkloadSlot(reusable, execute); }
  finally { fs.closeSync(reusable); }
  fs.chmodSync(directory, 0o755);
  assert.throws(() => openWorkloadSlot(directory), /private/);
  fs.chmodSync(directory, 0o700);
  fs.chmodSync(path.join(directory, "slot.lock"), 0o644);
  assert.throws(() => openWorkloadSlot(directory), /0600/);
  fs.chmodSync(path.join(directory, "slot.lock"), 0o600);
  fs.linkSync(path.join(directory, "slot.lock"), path.join(root, "alias"));
  assert.throws(() => openWorkloadSlot(directory), /hard links/);
  fs.unlinkSync(path.join(root, "alias"));
  fs.symlinkSync(directory, path.join(root, "directory-link"));
  assert.throws(() => openWorkloadSlot(path.join(root, "directory-link")), /private/);

  const parent = path.join(root, "hardgate-workload-ready-owned");
  fs.mkdirSync(parent, { mode: 0o700 });
  const ready = path.join(parent, "ready");
  acknowledge(ready);
  acknowledge(ready);
  fs.chmodSync(ready, 0o644);
  assert.throws(() => acknowledge(ready), /invalid inherited/);
  fs.unlinkSync(ready);
  fs.writeFileSync(path.join(root, "untouched"), "original");
  fs.symlinkSync(path.join(root, "untouched"), ready);
  assert.throws(() => acknowledge(ready), /invalid inherited/);
  assert.equal(fs.readFileSync(path.join(root, "untouched"), "utf8"), "original");
  assert.throws(() => acknowledge(path.join(root, "ready")), /owner-validated/);

  assert.equal(scopeActive("LoadState=not-found\n", "owned"), false);
  for (const state of ["active", "activating", "deactivating", "inactive", "failed"]) {
    const status = `LoadState=loaded\nDescription=owned\nActiveState=${state}\n`;
    assert.equal(scopeActive(status, "owned"), ["active", "activating", "deactivating"].includes(state));
    assert.throws(() => scopeActive(status, "foreign"), /different owner/);
  }
  assert.throws(() => scopeActive("LoadState=loaded\nDescription=owned\n", "owned"), /missing/);
  const args = launchArguments({ unit: "hardgate-workload-owned.scope", description: "owned" }, ["printf", "$HOME `literal`"], { quota: 200, memory: 4294967296, high: 3435973836 });
  assert.deepEqual(args.slice(-3), ["--", "printf", "$HOME `literal`"]);
  for (const required of ["--expand-environment=no", "--property=CPUQuota=200%", "--property=MemoryMax=4294967296", "--property=MemoryHigh=3435973836", "--property=MemorySwapMax=0", "--property=TasksMax=256", "--property=KillMode=control-group"]) assert.ok(args.includes(required), required);
} finally { fs.rmSync(root, { recursive: true, force: true }); }
console.log("resource_scope: shared locking, ownership, readiness and literal bounded launch verified");
