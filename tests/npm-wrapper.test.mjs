// Smoke test for the npm launcher. No deps. Run: node tests/npm-wrapper.test.mjs
// Verifies platform mapping, binary resolution (incl. pnpm-style layouts via
// require.resolve), and end-to-end `--version` passthrough.
"use strict";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const require = createRequire(import.meta.url);
const launcher = require("../npm/hardgate/bin/hardgate.js");

const primary = launcher.platformPackage();
assert.ok(primary, "platformPackage() must return a package name");
console.log(`platform: ${process.platform}/${process.arch} -> ${primary}`);

// 1. optionalDependencies cover every platform package on disk.
const mainPkg = JSON.parse(
  fs.readFileSync(path.join(root, "npm/hardgate/package.json"), "utf8"),
);
for (const dir of fs.readdirSync(path.join(root, "npm"))) {
  if (dir === "hardgate") continue;
  assert.ok(
    mainPkg.optionalDependencies?.[dir],
    `npm/hardgate must optionally depend on ${dir} (pnpm installs these by default)`,
  );
}

// 2. platform packages must declare os/cpu (+ libc on linux) so npm/pnpm
//    skip inapplicable binaries automatically.
for (const dir of Object.keys(mainPkg.optionalDependencies)) {
  const pkg = JSON.parse(
    fs.readFileSync(path.join(root, `npm/${dir}/package.json`), "utf8"),
  );
  assert.ok(pkg.os?.length, `${dir} needs package.json "os"`);
  assert.ok(pkg.cpu?.length, `${dir} needs package.json "cpu"`);
  if (pkg.os.includes("linux")) {
    assert.ok(
      pkg.libc?.length,
      `${dir} is linux and needs package.json "libc" (glibc|musl) for npm/pnpm filtering`,
    );
  }
}

// 3. End-to-end: launcher must exec the real binary and print a version.
const bin = launcher.findBinary();
assert.ok(bin, "findBinary() must locate a binary (dev fallback: target/release)");
console.log(`binary: ${bin}`);
const out = execFileSync(process.execPath, ["npm/hardgate/bin/hardgate.js", "--version"], {
  cwd: root,
  encoding: "utf8",
}).trim();
console.log(`--version: ${out}`);
assert.match(out, /hardgate \d+\.\d+\.\d+/);

console.log("npm-wrapper.test: OK");
