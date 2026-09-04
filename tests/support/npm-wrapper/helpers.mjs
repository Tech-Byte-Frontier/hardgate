import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const requireFromHelper = createRequire(import.meta.url);

export function repoRoot(metaUrl) {
  return path.join(path.dirname(fileURLToPath(metaUrl)), "..");
}

export function loadLauncher(file) {
  return requireFromHelper(file);
}

export function makeTempDir(prefix) {
  return fs.mkdtempSync(path.join(os.tmpdir(), prefix));
}

export function scrubbedPath(extra = []) {
  const dirs = new Set(["/usr/bin", "/bin", "/usr/local/bin", path.dirname(process.execPath)]);
  for (const dir of extra) dirs.add(dir);
  return [...dirs].join(path.delimiter);
}

export function runLauncher(
  launcher,
  args,
  { cwd, env = {}, nodeArgs = [], timeout = 15000 } = {},
) {
  return spawnSync(process.execPath, [...nodeArgs, launcher, ...args], {
    cwd,
    timeout,
    encoding: "utf8",
    env: {
      PATH: scrubbedPath(),
      HOME: os.homedir(),
      SYSTEMROOT: process.env.SYSTEMROOT,
      ...env,
    },
  });
}

export function copyLauncherAt(launcher, dir) {
  const file = path.join(dir, "node_modules/hardgate/bin/hardgate.js");
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.copyFileSync(launcher, file);
  return file;
}

export function makePathFixture(launcher, binary, prefix) {
  const dir = makeTempDir(prefix);
  const first = path.join(dir, "first");
  const later = path.join(dir, "later");
  fs.mkdirSync(first, { recursive: true });
  fs.mkdirSync(later, { recursive: true });
  fs.copyFileSync(binary, path.join(later, "hardgate"));
  return { dir, first, later, launcherCopy: copyLauncherAt(launcher, dir) };
}

export function writeExecutable(dir, name, contents) {
  const file = path.join(dir, name);
  fs.writeFileSync(file, contents, { mode: 0o755 });
  return file;
}

export function assertMissingBinary(result, packageName, label) {
  assert.equal(result.status, 1, `${label} must exit 1, got ${result.status}`);
  assert.match(result.stderr, /No prebuilt binary found/);
  assert.match(result.stderr, new RegExp(`expected optional dep '${packageName}'`));
}

export function assertPathVersion({ launcher, first, later, expected, label }) {
  const result = runLauncher(launcher, ["--version"], {
    env: { PATH: [first, later, "/usr/bin", "/bin"].join(path.delimiter) },
  });
  assert.equal(result.status, 0, `${label} must resolve a real binary`);
  assert.equal(result.stdout.trim(), expected);
}

export function realBinary(root) {
  for (const name of ["release", "debug"]) {
    const file = path.join(root, "target", name, "hardgate");
    if (fs.existsSync(file)) return file;
  }
  throw new Error("no dev binary: run `cargo build` first");
}

export function readVersion(binary) {
  return spawnSync(binary, ["--version"], { encoding: "utf8" }).stdout.trim();
}

export { fs, os, path, spawnSync };
