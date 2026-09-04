"use strict";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { runReleaseProcess } from "../scripts/release-process.mjs";

assert.equal(await runReleaseProcess(process.execPath, ["-e", "process.stdout.write('ok')"], { timeoutMs: 1000 }), "ok");
await assert.rejects(runReleaseProcess("hardgate-nonexistent-command", [], { timeoutMs: 1000 }), { code: "ENOENT" });
await assert.rejects(runReleaseProcess(process.execPath, ["-e", "process.stdout.write('excessive')"], { timeoutMs: 1000, maxBuffer: 2 }), /output limit/);
assert.throws(() => runReleaseProcess(process.execPath, [], { timeoutMs: 0 }), /positive integer/);

const directory = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-process-test-"));
try {
  const pidPath = path.join(directory, "child.pid");
  const source = `const fs = require('node:fs'); const {spawn} = require('node:child_process');
    const child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], {stdio: 'inherit'});
    fs.writeFileSync(process.argv[1], String(child.pid));
    process.on('SIGTERM', () => {}); setInterval(() => {}, 1000);`;
  const started = performance.now();
  await assert.rejects(runReleaseProcess(process.execPath, ["-e", source, pidPath], { timeoutMs: 500 }), /deadline/);
  assert.ok(performance.now() - started < 2000);
  const pid = Number(fs.readFileSync(pidPath));
  // An orphan may remain a zombie until the host init reaps it; a zombie has
  // exited and cannot execute, retain pipes, or write source files.
  let alive = false;
  try {
    process.kill(pid, 0);
    alive = process.platform !== "linux" || !fs.readFileSync(`/proc/${pid}/stat`, "utf8").includes(") Z ");
  } catch (error) { if (error.code !== "ESRCH" && error.code !== "ENOENT") throw error; }
  assert.equal(alive, false, "owned descendant must have terminated");
} finally {
  fs.rmSync(directory, { recursive: true, force: true });
}
console.log("release_process.test: OK");
