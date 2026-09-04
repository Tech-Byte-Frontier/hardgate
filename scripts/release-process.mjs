// Bound release subprocess output and lifetime, including owned descendants.
"use strict";
import { spawn } from "node:child_process";

function stopGroup(child) {
  if (!child.pid) return;
  try {
    if (process.platform === "win32") child.kill("SIGKILL");
    else process.kill(-child.pid, "SIGKILL");
  } catch (error) {
    if (error.code !== "ESRCH") throw error;
  }
}

function captureOutput(child, limit) {
  const output = { stdout: [], stderr: [], bytes: 0, exceeded: false };
  for (const stream of ["stdout", "stderr"]) {
    child[stream].on("data", (bytes) => {
      output.bytes += bytes.length;
      if (output.bytes <= limit) output[stream].push(bytes);
      else {
        output.exceeded = true;
        stopGroup(child);
      }
    });
  }
  return output;
}

export function runReleaseProcess(command, args, options = {}) {
  const { timeoutMs, maxBuffer = 4 * 1024 * 1024, ...spawnOptions } = options;
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1) throw new Error("subprocess timeoutMs must be a positive integer");
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { ...spawnOptions, detached: process.platform !== "win32", stdio: ["ignore", "pipe", "pipe"] });
    const output = captureOutput(child, maxBuffer);
    let failure;
    const timer = setTimeout(() => {
      failure = Object.assign(new Error(`${command} exceeded subprocess deadline`), { code: "ETIMEDOUT" });
      stopGroup(child);
    }, timeoutMs);
    child.on("error", (error) => { failure = error; });
    // A successful parent can leave descendants holding captured pipes open.
    child.on("exit", () => stopGroup(child));
    child.on("close", (status, signal) => {
      clearTimeout(timer);
      const stdout = Buffer.concat(output.stdout).toString("utf8");
      const stderr = Buffer.concat(output.stderr).toString("utf8");
      if (output.exceeded) failure = new Error(`${command} exceeded output limit`);
      if (!failure && status !== 0) failure = new Error(`${command} exited with status ${status}, signal ${signal}`);
      if (failure) reject(Object.assign(failure, { stdout, stderr, status }));
      else resolve(stdout);
    });
  });
}
