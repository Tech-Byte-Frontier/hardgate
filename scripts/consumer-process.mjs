"use strict";

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { bounded, fail } from "./consumer-schema.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const MAX_REPORT_OUTPUT = 1024 * 1024;

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
  const child = spawnSync(binary, args, { cwd, env: { ...process.env, ...env }, encoding: "utf8", timeout });
  const error = child.error;
  const timedOut = error?.code === "ETIMEDOUT";
  return {
    status: Number.isInteger(child.status) ? child.status : null,
    signal: child.signal ?? null,
    timedOut,
    spawnError: error && !timedOut ? bounded(`${error.code ?? "spawn"}: ${error.message}`) : null,
    stdout: bounded(child.stdout, MAX_REPORT_OUTPUT),
    stderr: bounded(child.stderr),
  };
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
