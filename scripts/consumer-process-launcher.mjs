"use strict";

import { spawn } from "node:child_process";
import fs from "node:fs";

const MAX_BYTES = Number(process.env.CONSUMER_LAUNCH_MAX_BYTES ?? 1024 * 1024);

function append(chunks, state, chunk) {
  const remaining = Math.max(0, MAX_BYTES - state.bytes);
  const kept = chunk.subarray(0, remaining);
  if (kept.length) chunks.push(kept);
  state.bytes += kept.length;
}

function writeResult(result) {
  const temporary = `${process.env.CONSUMER_LAUNCH_RESULT}.tmp`;
  fs.writeFileSync(temporary, JSON.stringify(result));
  fs.renameSync(temporary, process.env.CONSUMER_LAUNCH_RESULT);
}

function launch() {
  const stdout = [];
  const stderr = [];
  const stdoutState = { bytes: 0 };
  const stderrState = { bytes: 0 };
  let recorded = false;
  const record = (result) => {
    if (recorded) return;
    recorded = true;
    writeResult(result);
  };
  const args = JSON.parse(process.env.CONSUMER_LAUNCH_ARGS);
  const env = JSON.parse(process.env.CONSUMER_LAUNCH_ENV);
  const child = spawn(process.env.CONSUMER_LAUNCH_BINARY, args, {
    cwd: process.env.CONSUMER_LAUNCH_CWD,
    env,
    stdio: ["ignore", "pipe", "pipe"],
  });
  child.stdout.on("data", (chunk) => append(stdout, stdoutState, chunk));
  child.stderr.on("data", (chunk) => append(stderr, stderrState, chunk));
  child.on("error", (error) => record({
    status: null,
    signal: null,
    spawnError: `${error.code ?? "spawn"}: ${error.message}`,
    stdout: Buffer.concat(stdout).toString("utf8"),
    stderr: Buffer.concat(stderr).toString("utf8"),
  }));
  child.on("close", (status, signal) => record({
    status,
    signal,
    spawnError: null,
    stdout: Buffer.concat(stdout).toString("utf8"),
    stderr: Buffer.concat(stderr).toString("utf8"),
  }));
}

try {
  launch();
} catch (error) {
  writeResult({
    status: null,
    signal: null,
    spawnError: `${error.code ?? "launcher"}: ${error.message}`,
    stdout: "",
    stderr: "",
  });
}
