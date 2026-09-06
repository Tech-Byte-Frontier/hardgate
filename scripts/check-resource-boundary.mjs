#!/usr/bin/env node
// Require real cgroup limits before repository maintenance tools start.
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const read = (file) => fs.readFileSync(file, "utf8").trim();
function integer(value) {
  if (!/^\d+$/.test(value) || !Number.isSafeInteger(Number(value))) throw new Error("invalid resource integer");
  return Number(value);
}

export function memoryLimit(proc = "/proc") {
  const value = read(path.join(proc, "meminfo")).match(/^MemTotal:\s+(\d+)\s+kB$/m)?.[1];
  if (!value) throw new Error("missing host memory size");
  return Math.min(Math.floor(integer(value) * 1024 / 4), 4 * 1024 ** 3);
}

export function bounded(directory, limit) {
  try {
    const value = (name) => read(path.join(directory, name));
    const tasks = integer(value("pids.max"));
    return boundedCpu(value("cpu.max")) && boundedMemory(value, limit)
      && tasks > 0 && tasks <= 256;
  } catch { return false; }
}

function boundedCpu(value) {
  const [quota, period, extra] = value.split(/\s+/).map(integer);
  return extra === undefined && quota > 0 && period > 0 && quota <= 2 * period;
}

function boundedMemory(value, limit) {
  const maximum = integer(value("memory.max")), high = integer(value("memory.high"));
  return maximum > 0 && maximum <= limit && high > 0
    && high <= Math.floor(maximum / 5) * 4 && value("memory.swap.max") === "0";
}

export function boundary(proc = "/proc", mount = "/sys/fs/cgroup") {
  const limit = memoryLimit(proc);
  const membership = read(path.join(proc, "self/cgroup")).split("\n").find((line) => line.startsWith("0::"))?.slice(3);
  if (!membership?.startsWith("/") || membership.split("/").includes("..")) throw new Error("invalid cgroup membership");
  let current = path.join(mount, membership);
  while (current !== mount) {
    if (bounded(current, limit)) return current;
    current = path.dirname(current);
  }
  if (bounded(mount, limit)) return mount;
  throw new Error("no enforced CPU, memory, swap and task boundary");
}

export function eventCounters(directory, name) {
  const counters = new Map();
  for (const line of read(path.join(directory, name)).split("\n")) {
    const [key, value, extra] = line.split(/\s+/);
    if (!key || extra !== undefined || counters.has(key)) throw new Error("invalid resource event counter");
    counters.set(key, integer(value));
  }
  const required = name === "memory.events" ? ["max", "oom", "oom_kill"] : ["max"];
  if (required.some((key) => !counters.has(key))) throw new Error("missing resource event counters");
  return counters;
}

function affinityCount() {
  const list = read("/proc/self/status").match(/^Cpus_allowed_list:\s+([\d,-]+)$/m)?.[1];
  if (!list) throw new Error("missing CPU affinity");
  return list.split(",").reduce((count, range) => {
    const [first, last = first] = range.split("-").map(integer);
    if (last < first) throw new Error("invalid CPU affinity");
    return count + last - first + 1;
  }, 0);
}

function main(args) {
  if (args.length === 1 && args[0] === "--limits") {
    const memory = memoryLimit();
    console.log(`${Math.min(200, affinityCount() * 50)} ${memory} ${Math.floor(memory / 5) * 4}`);
    return;
  }
  const directory = boundary();
  if (args.length === 1 && args[0] === "--events") {
    for (const name of ["memory.events", "pids.events"]) {
      for (const [key, value] of [...eventCounters(directory, name)].sort(([a], [b]) => a.localeCompare(b))) {
        if (!["low", "high", "sock_throttled"].includes(key)) console.log(`${name} ${key} ${value}`);
      }
    }
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try { main(process.argv.slice(2)); }
  catch (error) { console.error(`hardgate workload limits: ${error.message}`); process.exitCode = 2; }
}
