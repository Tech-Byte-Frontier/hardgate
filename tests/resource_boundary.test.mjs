import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { boundary, bounded, eventCounters } from "../scripts/check-resource-boundary.mjs";
const root = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-resource-script-"));
try {
  const proc = path.join(root, "proc"), mount = path.join(root, "cgroup"), scope = path.join(mount, "parent/workload");
  fs.mkdirSync(path.join(proc, "self"), { recursive: true });
  fs.mkdirSync(scope, { recursive: true });
  fs.writeFileSync(path.join(proc, "meminfo"), "MemTotal:       16777216 kB\n");
  fs.writeFileSync(path.join(proc, "self/cgroup"), "0::/parent/workload\n");
  const controls = { "cpu.max": "200000 100000", "memory.max": "4294967296", "memory.high": "3435973836", "memory.swap.max": "0", "pids.max": "256" };
  const write = (values) => { for (const [name, value] of Object.entries(values)) fs.writeFileSync(path.join(scope, name), value); };
  write(controls);
  assert.equal(boundary(proc, mount), scope);
  for (const [name, value] of [["cpu.max", "max 100000"], ["cpu.max", "300000 100000"], ["memory.max", "8589934592"], ["memory.high", "4294967296"], ["memory.swap.max", "1"], ["pids.max", "max"]]) {
    write({ [name]: value });
    assert.equal(bounded(scope, 4294967296), false, name);
    assert.throws(() => boundary(proc, mount), /no enforced/);
    write(controls);
  }
  fs.writeFileSync(path.join(scope, "memory.events"), "low 0\nhigh 12\nmax 0\noom 0\noom_kill 0\n");
  assert.equal(eventCounters(scope, "memory.events").get("high"), 12);
  for (const text of ["max 0\n", "max 0\noom 0\noom_kill -1\n", "max 0\nmax 1\noom 0\noom_kill 0\n"]) {
    fs.writeFileSync(path.join(scope, "memory.events"), text);
    assert.throws(() => eventCounters(scope, "memory.events"));
  }
  fs.writeFileSync(path.join(proc, "self/cgroup"), "0::/../outside\n");
  assert.throws(() => boundary(proc, mount), /invalid cgroup/);
} finally { fs.rmSync(root, { recursive: true, force: true }); }
console.log("resource_boundary: enforced controls and complete event evidence verified");
