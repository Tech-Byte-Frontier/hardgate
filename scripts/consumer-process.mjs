"use strict";

import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { bounded, fail } from "./consumer-schema.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const MAX_REPORT_OUTPUT = 1024 * 1024;
const PROCESS_POLL_MS = 5;
const TERMINATION_GRACE_MS = 150;
const LAUNCHER = path.join(path.dirname(fileURLToPath(import.meta.url)), "consumer-process-launcher.mjs");

function isExecutableFile(candidate) {
  try {
    const stat = fs.statSync(candidate);
    return stat.isFile() && (stat.mode & 0o111) !== 0;
  } catch {
    return false;
  }
}

export function resolveBinary(options = {}) {
  const explicit = Object.prototype.hasOwnProperty.call(options, "binary") && options.binary !== undefined;
  if (explicit) return validateBinary(options.binary, "explicit --binary");
  if (process.env.HARDGATE_BINARY) return validateBinary(process.env.HARDGATE_BINARY, "HARDGATE_BINARY");
  const candidates = [path.join(ROOT, "target", "debug", "hardgate"), path.join(ROOT, "target", "release", "hardgate")];
  const found = candidates.find((candidate) => isExecutableFile(candidate));
  if (!found) fail("binary-missing", "No executable local Hardgate binary found; build hardgate or set HARDGATE_BINARY.");
  return path.resolve(found);
}

function validateBinary(candidate, label) {
  if (typeof candidate !== "string" || candidate.length === 0) fail("binary-invalid", `${label} must be a non-empty path`);
  const resolved = path.resolve(candidate);
  let stat;
  try {
    stat = fs.statSync(resolved);
  } catch {
    fail("binary-missing", `${label} does not exist: ${resolved}`);
  }
  if (!stat.isFile()) fail("binary-invalid", `${label} must be a regular file: ${resolved}`);
  if ((stat.mode & 0o111) === 0) fail("binary-invalid", `${label} must be executable: ${resolved}`);
  return resolved;
}

export function runProcess({ binary, args, cwd, env = {}, timeout = 30_000 }) {
  const processRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-consumer-process-"));
  let result;
  let primaryError;
  try {
    result = runProcessBody({ binary, args, cwd, env, timeout, processRoot });
  } catch (error) {
    primaryError = error;
  }
  try {
    fs.rmSync(processRoot, { recursive: true, force: true });
  } catch (error) {
    const detail = `process temp cleanup failed: ${error.message}`;
    if (primaryError) primaryError.message = `${primaryError.message}; ${detail}`;
    else result = processResult({ spawnError: detail });
  }
  if (primaryError) throw primaryError;
  return result;
}

function runProcessBody({ binary, args, cwd, env, timeout, processRoot }) {
  const resultPath = path.join(processRoot, "result.json");
  const childEnv = { ...process.env, ...env };
  let launcher;
  try {
    launcher = spawn(process.execPath, [LAUNCHER], {
      cwd: ROOT,
      detached: process.platform !== "win32",
      stdio: "ignore",
      env: {
        ...childEnv,
        CONSUMER_LAUNCH_BINARY: binary,
        CONSUMER_LAUNCH_ARGS: JSON.stringify(args),
        CONSUMER_LAUNCH_CWD: cwd,
        CONSUMER_LAUNCH_ENV: JSON.stringify(childEnv),
        CONSUMER_LAUNCH_RESULT: resultPath,
        CONSUMER_LAUNCH_MAX_BYTES: String(MAX_REPORT_OUTPUT),
      },
    });
  } catch (error) {
    return processResult({ spawnError: `${error.code ?? "spawn"}: ${error.message}` });
  }
  try {
    const completed = waitForResult(resultPath, launcher.pid, timeout);
    if (!completed) terminateProcessGroup(launcher.pid);
    return completed ? readProcessResult(resultPath) : processResult({ timedOut: true });
  } catch (error) {
    terminateProcessGroup(launcher.pid);
    throw error;
  }
}

function processResult(overrides = {}) {
  return { status: null, signal: null, timedOut: false, spawnError: null, stdout: "", stderr: "", ...overrides };
}

function waitForResult(resultPath, pid, timeout) {
  const deadline = Date.now() + Math.max(1, timeout);
  while (!fs.existsSync(resultPath) && Date.now() < deadline) {
    if (pid && !processAlive(pid)) break;
    sleep(PROCESS_POLL_MS);
  }
  return fs.existsSync(resultPath);
}

function readProcessResult(resultPath) {
  try {
    const result = JSON.parse(fs.readFileSync(resultPath, "utf8"));
    return processResult({
      status: Number.isInteger(result.status) ? result.status : null,
      signal: typeof result.signal === "string" ? result.signal : null,
      spawnError: result.spawnError ? bounded(result.spawnError) : null,
      stdout: bounded(result.stdout, MAX_REPORT_OUTPUT),
      stderr: bounded(result.stderr),
    });
  } catch (error) {
    return processResult({ spawnError: bounded(`launcher-result: ${error.message}`) });
  }
}

function sleep(milliseconds) {
  const buffer = new SharedArrayBuffer(4);
  Atomics.wait(new Int32Array(buffer), 0, 0, milliseconds);
}

function processAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

function terminateProcessGroup(pid) {
  if (!pid) return;
  if (process.platform === "win32") {
    try { process.kill(pid, "SIGTERM"); } catch { return; }
    waitForExit(pid, TERMINATION_GRACE_MS);
    try { process.kill(pid, "SIGKILL"); } catch { /* already exited */ }
    return;
  }
  signalGroup(pid, "SIGTERM");
  waitForExit(pid, TERMINATION_GRACE_MS);
  signalGroup(pid, "SIGKILL");
  waitForExit(pid, TERMINATION_GRACE_MS);
}

function signalGroup(pid, signal) {
  try { process.kill(-pid, signal); } catch { /* group already exited */ }
}

function waitForExit(pid, timeout) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline && processAlive(pid)) sleep(PROCESS_POLL_MS);
}

export function processFailure(result, expectedExit, label) {
  if (result.spawnError) return ["spawn-error", `${label} process spawn error: ${result.spawnError}`];
  if (result.timedOut) return ["timeout", `${label} timed out`];
  if (result.signal) return ["signal", `${label} terminated by signal ${result.signal}`];
  if (result.status === null) return ["no-exit-status", `${label} did not provide an exit status`];
  if (result.status !== expectedExit) return ["exit-status-mismatch", `${label} exited ${result.status}; expected ${expectedExit}`];
  return null;
}

export function failureResult(code, diagnostics, result = {}, extra = {}) {
  return {
    status: "fail", reasonCode: code, diagnostics: bounded(diagnostics), exitCode: result.status ?? null,
    signal: result.signal ?? null, timedOut: Boolean(result.timedOut), report: null,
    ...extra,
  };
}
