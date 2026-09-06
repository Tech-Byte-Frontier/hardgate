#!/usr/bin/env node
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawn, execFile } from "node:child_process";
import { promisify } from "node:util";
import { pathToFileURL } from "node:url";
import { resourceLimits } from "./check-resource-boundary.mjs";
import { openWorkloadSlot, acquireWorkloadSlot } from "./resource-scope-lock.mjs";

const control = promisify(execFile);
const READY = "HARDGATE_RESOURCE_SCRIPT_READY";

function managerEnvironment() {
  const environment = { ...process.env, XDG_RUNTIME_DIR: `/run/user/${process.getuid()}` };
  delete environment.DBUS_SESSION_BUS_ADDRESS;
  return environment;
}

export function launchArguments(scope, command, limits) {
  const properties = {
    CPUQuota: `${limits.quota}%`, CPUWeight: 25,
    MemoryMax: limits.memory, MemoryHigh: limits.high, MemorySwapMax: 0,
    TasksMax: 256, OOMPolicy: "kill", KillMode: "control-group",
    TimeoutStopSec: "1s", RuntimeMaxSec: "1800s",
  };
  return ["--user", "--scope", "--quiet", "--collect", "--expand-environment=no",
    `--unit=${scope.unit}`, `--description=${scope.description}`,
    ...Object.entries(properties).map(([key, value]) => `--property=${key}=${value}`), "--", ...command];
}

export function scopeActive(output, expected) {
  const fields = Object.fromEntries(output.trim().split("\n").map((line) => {
    const separator = line.indexOf("=");
    return [line.slice(0, separator), line.slice(separator + 1)];
  }));
  if (fields.LoadState === "not-found") return false;
  if (fields.Description !== expected) throw new Error("workload scope has a different owner identity");
  if (["inactive", "failed"].includes(fields.ActiveState)) return false;
  if (["active", "activating", "deactivating"].includes(fields.ActiveState)) return true;
  throw new Error("missing workload scope state");
}

async function manager(args) {
  const { stdout } = await control("systemctl", ["--user", ...args], { env: managerEnvironment(), timeout: 5000 });
  return stdout;
}

async function stopScope(scope) {
  const inspect = async () => scopeActive(await manager(["show", scope.unit, "--property=LoadState,ActiveState,Description"]), scope.description);
  if (!await inspect()) return;
  await manager(["stop", scope.unit]);
  if (await inspect()) throw new Error("owned workload scope remains active after cleanup");
}

export function acknowledge(ready = process.env[READY]) {
  if (!ready) return;
  validateReadinessDirectory(ready);
  try { fs.closeSync(fs.openSync(ready, "wx", 0o600)); }
  catch (error) { if (error.code !== "EEXIST") throw error; }
  const marker = fs.lstatSync(ready);
  if (!marker.isFile() || marker.uid !== process.getuid() || marker.nlink !== 1 || (marker.mode & 0o7777) !== 0o600) {
    throw new Error("invalid inherited workload readiness marker");
  }
}

function validateReadinessDirectory(ready) {
  const parent = path.dirname(ready), metadata = fs.lstatSync(parent);
  if (path.basename(ready) !== "ready" || !path.basename(parent).startsWith("hardgate-workload-ready-")
    || !metadata.isDirectory() || metadata.uid !== process.getuid() || (metadata.mode & 0o7777) !== 0o700) {
    throw new Error("workload readiness directory is not owner-validated");
  }
}

function ownedScope() {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), `hardgate-workload-ready-${process.pid}-`));
  const identity = path.basename(directory).slice("hardgate-workload-ready-".length);
  return { directory, ready: path.join(directory, "ready"), unit: `hardgate-workload-${identity}.scope`, description: `Hardgate workload ${directory}` };
}

function execution() {
  const state = { child: undefined, signal: undefined };
  const handlers = ["SIGINT", "SIGTERM"].map((signal) => {
    const handler = () => { state.signal ??= signal; state.child?.kill("SIGTERM"); };
    process.on(signal, handler);
    return [signal, handler];
  });
  const execute = (program, args, options = {}) => new Promise((resolve, reject) => {
    if (state.signal) { reject(new Error("workload cancelled")); return; }
    const child = spawn(program, args, { stdio: "inherit", ...options });
    state.child = child;
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      state.child = undefined;
      if (state.signal || signal) reject(new Error("workload cancelled or terminated; evidence is incomplete"));
      else resolve(code);
    });
  });
  return { state, execute, close: () => { for (const [signal, handler] of handlers) process.off(signal, handler); } };
}

async function supervise(command, runner) {
  const fd = openWorkloadSlot();
  let scope;
  try {
    await acquireWorkloadSlot(fd, runner.execute);
    scope = ownedScope();
    const environment = { ...managerEnvironment(), HARDGATE_RESOURCE_SCRIPT_CHILD: "1", [READY]: scope.ready };
    // The scope leader also holds the shared open-file description, so killing
    // this supervisor cannot release the slot while its workload is still live.
    const status = await runner.execute("systemd-run", launchArguments(scope, command, resourceLimits()), { env: environment, stdio: ["inherit", "inherit", "inherit", fd] });
    if (!fs.existsSync(scope.ready)) throw new Error("the systemd user manager did not establish the owned workload scope; no workload was acknowledged");
    return status;
  } finally {
    try { if (scope) await stopScope(scope); }
    finally {
      if (scope) { fs.rmSync(scope.ready, { force: true }); fs.rmdirSync(scope.directory); }
      fs.closeSync(fd);
    }
  }
}

async function main(args) {
  if (args.length === 1 && args[0] === "--ack") { acknowledge(); return; }
  if (!args.length) throw new Error("expected the resource-limited command");
  const runner = execution();
  try { process.exitCode = await supervise(args, runner); }
  catch (error) { console.error(`hardgate: ${error.message}`); process.exitCode = runner.state.signal === "SIGINT" ? 130 : runner.state.signal ? 143 : 2; }
  finally { runner.close(); }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main(process.argv.slice(2)).catch((error) => { console.error(`hardgate: ${error.message}`); process.exitCode = 2; });
}
