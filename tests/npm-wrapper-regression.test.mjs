// Regression tests for the npm launcher's self-execution guard.
//
// Background: when the platform binary is absent but a `hardgate` wrapper
// shim sits on PATH (e.g. a `pnpm dlx` sandbox with `.bin` first on PATH),
// the PATH fallback used to resolve the shim itself and re-exec it with the
// same argv -> unbounded self-spawn until the kernel refused with E2BIG.
// These tests pin the fix: wrapper scripts are never accepted as binaries,
// a recursion fuse aborts nesting, and argv[1]-logical resolution finds
// platform packages across pnpm-style symlinked layouts.
//
// Mutation oracle: run
//   hardgate mutate --scoped npm/hardgate/bin/hardgate.js \
//     --test-cmd "node tests/npm-wrapper-regression.test.mjs"
// (the default `pnpm test` has no script and is a vacuous oracle).
// Accepted survivors, each verified by mutant-vs-test battery, not by
// reasoning alone:
//   - `acceptCandidate` `||` -> `&&`: equivalent on all 8 input classes
//     (missing/self/symlink-self/real-binary/text/dir/dangling). Self paths
//     are always JS text so the downstream magic-byte gate rejects them
//     anyway; the `||` is defense-in-depth short-circuiting. Deliberately
//     NOT covered by a test: a test passing under both versions is theater.
//   - `acceptCandidate` catch `return false`: needs fault injection
//     (TOCTOU/EACCES race between existsSync and statSync) to trigger;
//     testing it would mean mocking fs or racy chmod games.
//   - fuse-breaking mutants hang until timeout: that IS the fail-closed
//     signal (`reject_timeouts` keeps the gate red), so they are recorded
//     as timeouts, not clean kills.
//
// Run: node tests/npm-wrapper-regression.test.mjs
"use strict";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const launcher = path.join(root, "npm/hardgate/bin/hardgate.js");

function realBinary() {
  for (const p of [
    path.join(root, "target/release/hardgate"),
    path.join(root, "target/debug/hardgate"),
  ]) {
    if (fs.existsSync(p)) return p;
  }
  throw new Error("no dev binary: run `cargo build` first");
}

function mkdtemp(prefix) {
  return fs.mkdtempSync(path.join(os.tmpdir(), prefix));
}

// Minimal PATH that can still run `node` + system basics but contains NO
// hardgate binary and NO cargo bin (proves no reliance on ambient installs).
function scrubbedPath(extra = []) {
  const keep = new Set(["/usr/bin", "/bin", "/usr/local/bin"]);
  const nodeDir = path.dirname(process.execPath);
  keep.add(nodeDir);
  for (const d of extra) keep.add(d);
  return [...keep].join(path.delimiter);
}

function runLauncher(args, { cwd, env = {}, timeout = 15000 } = {}) {
  return spawnSync(process.execPath, [launcher, ...args], {
    cwd,
    timeout,
    encoding: "utf8",
    env: {
      // Start from a scrubbed environment: no PATH fallback surprises,
      // no HARDGATE_* overrides leaking in from the developer shell.
      PATH: scrubbedPath(),
      HOME: os.homedir(),
      SYSTEMROOT: process.env.SYSTEMROOT,
      ...env,
    },
  });
}

const expectedVersion = spawnSync(realBinary(), ["--version"], {
  encoding: "utf8",
}).stdout.trim();
assert.match(expectedVersion, /hardgate \d+\.\d+\.\d+/);

// --- A. shell-shim on PATH, no platform dep anywhere -> clean exit 1 ---
{
  const dir = mkdtemp("hg-shim-");
  const fakeBinDir = path.join(dir, "bin");
  fs.mkdirSync(fakeBinDir, { recursive: true });
  // A pnpm-style `.bin/hardgate` shell shim pointing at this launcher
  // (i.e. what the launcher ITSELF looks like when found via PATH).
  fs.writeFileSync(
    path.join(fakeBinDir, "hardgate"),
    `#!/bin/sh\nexec "${process.execPath}" "${launcher}" "$@"\n`,
    { mode: 0o755 },
  );
  // Hide the repo dev fallback: run with cwd elsewhere is not enough
  // (step 3 is __dirname-relative), so also shadow target/ by pointing
  // argv[1] at a COPY of the launcher outside the repo.
  const outside = path.join(dir, "node_modules/hardgate/bin");
  fs.mkdirSync(outside, { recursive: true });
  const launcherCopy = path.join(outside, "hardgate.js");
  fs.copyFileSync(launcher, launcherCopy);
  const res = spawnSync(process.execPath, [launcherCopy, "--version"], {
    timeout: 15000,
    encoding: "utf8",
    env: {
      PATH: scrubbedPath([fakeBinDir]),
      HOME: os.homedir(),
    },
  });
  assert.equal(
    res.status,
    1,
    `shim-on-PATH must exit 1, got ${res.status}: ${res.stderr}`,
  );
  assert.match(res.stderr, /No prebuilt binary found/);
  // The message must name the expected optional dep: pins the
  // `platformPackage() || "<unknown-platform>"` branch (serving
  // "<unknown-platform>" on a supported platform is a user-facing lie).
  {
    const { createRequire: createRequireA } = await import("node:module");
    const { platformPackage: platformPackageA } = createRequireA(
      import.meta.url,
    )(launcherCopy);
    assert.ok(
      res.stderr.includes(`expected optional dep '${platformPackageA()}'`),
      `stderr must name expected dep, got: ${res.stderr}`,
    );
  }
  console.log("A: shim-on-PATH degrades to clean exit 1 -- OK");
}

// --- B. recursion fuse: HARDGATE_BINARY pointing at the launcher itself ---
// Bound at 8s: the fuse trips in ~1s on good code (6 nested spawns), and a
// tight bound keeps this test a fast failure rather than a hang when the
// depth logic is broken. Must stay below the mutation per-mutant timeout
// (10s) so fuse-breaking mutants report as killed, not timeouts.
{
  const res = runLauncher(["--version"], {
    env: { HARDGATE_BINARY: launcher, PATH: scrubbedPath() },
    timeout: 8000,
  });
  // Either the fuse trips (depth already >5? no, depth 0 here) -- the
  // launcher execs itself with depth+1 until the fuse trips at depth 6.
  assert.equal(res.status, 1, `fuse must exit 1, got ${res.status}`);
  assert.match(res.stderr ?? res.stdout, /[Rr]ecurs/);
  console.log("B: self-exec fuse trips with clear error -- OK");
}

// --- D. dlx shape: root symlink present, binary file removed, shim on PATH ---
// (exact repro of the observed fork-bomb: must exit 1 quickly, never loop)
{
  const dir = mkdtemp("hg-dlx-");
  const nm = path.join(dir, "node_modules");
  const dotbin = path.join(nm, ".bin");
  fs.mkdirSync(dotbin, { recursive: true });
  // Consumer root with a platform package dir whose binary is MISSING
  // (unresolvable optional dep) plus a .bin shim first on PATH.
  const platDir = path.join(nm, "hardgate-linux-x64");
  fs.mkdirSync(path.join(platDir, "bin"), { recursive: true });
  fs.writeFileSync(
    path.join(platDir, "package.json"),
    JSON.stringify({ name: "hardgate-linux-x64", version: "0.0.0-test" }),
  );
  const mainDir = path.join(nm, "hardgate/bin");
  fs.mkdirSync(mainDir, { recursive: true });
  fs.copyFileSync(launcher, path.join(mainDir, "hardgate.js"));
  fs.writeFileSync(
    path.join(dotbin, "hardgate"),
    `#!/bin/sh\nexec "${process.execPath}" "${path.join(mainDir, "hardgate.js")}" "$@"\n`,
    { mode: 0o755 },
  );
  const res = spawnSync(
    process.execPath,
    [path.join(mainDir, "hardgate.js"), "--version"],
    {
      timeout: 15000,
      encoding: "utf8",
      cwd: os.tmpdir(),
      env: { PATH: scrubbedPath([dotbin]), HOME: os.homedir() },
    },
  );
  assert.equal(
    res.status,
    1,
    `dlx-without-binary must exit 1 (not loop), got ${res.status}`,
  );
  assert.match(res.stderr, /No prebuilt binary found/);
  {
    const { createRequire: createRequireD } = await import("node:module");
    const { platformPackage: platformPackageD } = createRequireD(
      import.meta.url,
    )(path.join(mainDir, "hardgate.js"));
    assert.ok(
      res.stderr.includes(`expected optional dep '${platformPackageD()}'`),
      `stderr must name expected dep, got: ${res.stderr}`,
    );
  }
  console.log("D: dlx-without-binary exits 1 without recursion -- OK");
}

// --- E. a shell script earlier on PATH is skipped for a later real binary ---
{
  const dir = mkdtemp("hg-magic-");
  const fakeFirst = path.join(dir, "first");
  const realLater = path.join(dir, "later");
  fs.mkdirSync(fakeFirst, { recursive: true });
  fs.mkdirSync(realLater, { recursive: true });
  fs.writeFileSync(
    path.join(fakeFirst, "hardgate"),
    `#!/bin/sh\necho "SHOULD-NEVER-RUN"\n`,
    { mode: 0o755 },
  );
  fs.copyFileSync(realBinary(), path.join(realLater, "hardgate"));
  const outside = path.join(dir, "node_modules/hardgate/bin");
  fs.mkdirSync(outside, { recursive: true });
  const launcherCopy = path.join(outside, "hardgate.js");
  fs.copyFileSync(launcher, launcherCopy);
  const res = spawnSync(process.execPath, [launcherCopy, "--version"], {
    timeout: 15000,
    encoding: "utf8",
    env: {
      PATH: [fakeFirst, realLater, "/usr/bin", "/bin"].join(path.delimiter),
      HOME: os.homedir(),
    },
  });
  assert.equal(res.status, 0, `should skip script, use ELF: ${res.stderr}`);
  assert.equal(res.stdout.trim(), expectedVersion);
  console.log("E: PATH script skipped in favor of real binary -- OK");
}

// --- G. platform matrix (+musl fallback mapping) is pinned ---
{
  const { createRequire } = await import("node:module");
  const requireG = createRequire(import.meta.url);
  const { resolvePlatform, fallbackPackages, detectMusl } = requireG(
    "../npm/hardgate/bin/hardgate.js",
  );
  const matrix = [
    ["linux", "x64", false, "hardgate-linux-x64"],
    ["linux", "x64", true, "hardgate-linux-x64-musl"],
    ["linux", "arm64", false, "hardgate-linux-arm64"],
    ["linux", "arm64", true, "hardgate-linux-arm64-musl"],
    ["darwin", "x64", null, "hardgate-darwin-x64"],
    ["darwin", "x64", false, "hardgate-darwin-x64"],
    ["darwin", "arm64", null, "hardgate-darwin-arm64"],
    ["win32", "x64", null, "hardgate-win32-x64"],
    ["win32", "arm64", null, null],
    ["linux", "s390x", false, null],
    ["freebsd", "x64", null, null],
  ];
  for (const [platform, arch, musl, expected] of matrix) {
    assert.equal(
      resolvePlatform(platform, arch, musl),
      expected,
      `${platform}/${arch} musl=${musl}`,
    );
  }
  assert.deepEqual(fallbackPackages("hardgate-linux-x64"), [
    "hardgate-linux-x64-musl",
  ]);
  assert.deepEqual(fallbackPackages("hardgate-linux-x64-musl"), [
    "hardgate-linux-x64",
  ]);
  assert.deepEqual(fallbackPackages("hardgate-linux-arm64"), [
    "hardgate-linux-arm64-musl",
  ]);
  assert.deepEqual(fallbackPackages("hardgate-darwin-arm64"), []);
  assert.deepEqual(fallbackPackages("hardgate-win32-x64"), []);
  console.log("G: platform matrix + musl fallbacks pinned -- OK");
}

// --- H. musl-detection truth table (glibc wins, alpine decides otherwise) ---
{
  const { createRequire } = await import("node:module");
  const requireH = createRequire(import.meta.url);
  const { detectMusl } = requireH("../npm/hardgate/bin/hardgate.js");
  const cases = [
    // [platform, glibcVersionRuntime, alpineRelease, expected]
    ["linux", "2.39", false, false],
    ["linux", "2.39", true, false],
    ["linux", null, true, true],
    ["linux", null, false, false],
    ["linux", undefined, true, true],
    ["darwin", null, false, false],
    ["darwin", null, true, false],
    ["win32", null, true, false],
    ["freebsd", null, false, false],
  ];
  for (const [platform, ver, alpine, expected] of cases) {
    assert.equal(
      detectMusl(platform, ver, alpine),
      expected,
      `detectMusl(${platform}, ${ver}, ${alpine})`,
    );
  }
  console.log("H: musl-detection truth table -- OK");
}

// --- I. isRealBinary accepts only machine binaries (magic bytes) ---
{
  const { createRequire } = await import("node:module");
  const requireI = createRequire(import.meta.url);
  const { isRealBinary } = requireI("../npm/hardgate/bin/hardgate.js");
  const dir = mkdtemp("hg-magic-");
  const files = [
    ["elf", Buffer.from([0x7f, 0x45, 0x4c, 0x46]), true],
    ["elf-truncated", Buffer.from([0x7f, 0x45]), false],
    ["elf-lookalike", Buffer.from([0x7f, 0x4f, 0x4f, 0x4f]), false],
    // Partial ELF magic must NOT pass: every `&&` in the header chain is
    // load-bearing (a widened `||` would let a script masquerade as a
    // machine binary and get executed). These two rows kill the `&&`->`||`
    // mutants on the middle and tail of the chain.
    ["elf-first-two", Buffer.from([0x7f, 0x45, 0x00, 0x00]), false],
    ["elf-first-three", Buffer.from([0x7f, 0x45, 0x4c, 0x00]), false],
    ["mz", Buffer.from([0x4d, 0x5a, 0x90, 0x00]), true],
    ["mz-lookalike", Buffer.from([0x4d, 0x00, 0x00, 0x00]), false],
    ["macho-le64", Buffer.from([0xcf, 0xfa, 0xed, 0xfe]), true],
    ["macho-fat", Buffer.from([0xca, 0xfe, 0xba, 0xbe]), true],
    ["shell", Buffer.from("#!/bin/sh\necho hi\n"), false],
    ["empty", Buffer.alloc(0), false],
    ["text", Buffer.from("not a binary"), false],
  ];
  for (const [name, bytes, expected] of files) {
    const p = path.join(dir, name);
    fs.writeFileSync(p, bytes);
    assert.equal(isRealBinary(p), expected, `isRealBinary(${name})`);
  }
  assert.equal(isRealBinary(path.join(dir, "missing")), false);
  console.log("I: binary magic-byte gate -- OK");
}

// --- I2. isSelf recognizes the launcher itself, tolerates broken links ---
{
  const { createRequire } = await import("node:module");
  const requireI2 = createRequire(import.meta.url);
  const { isSelf } = requireI2("../npm/hardgate/bin/hardgate.js");
  const dir = mkdtemp("hg-self-");
  assert.equal(isSelf(launcher), true);
  assert.equal(isSelf(realBinary()), false);
  const dangling = path.join(dir, "dangling");
  fs.symlinkSync(path.join(dir, "no-such-target"), dangling);
  assert.equal(isSelf(dangling), false);
  assert.equal(isSelf(path.join(dir, "missing")), false);
  console.log("I2: self-recognition + broken-link tolerance -- OK");
}

// --- J. a directory named `hardgate` on PATH is skipped ---
{
  const dir = mkdtemp("hg-dirskip-");
  const dirFirst = path.join(dir, "first");
  const realLater = path.join(dir, "later");
  fs.mkdirSync(path.join(dirFirst, "hardgate"), { recursive: true });
  fs.mkdirSync(realLater, { recursive: true });
  fs.copyFileSync(realBinary(), path.join(realLater, "hardgate"));
  const outside = path.join(dir, "node_modules/hardgate/bin");
  fs.mkdirSync(outside, { recursive: true });
  const launcherCopy = path.join(outside, "hardgate.js");
  fs.copyFileSync(launcher, launcherCopy);
  const res = spawnSync(process.execPath, [launcherCopy, "--version"], {
    timeout: 15000,
    encoding: "utf8",
    env: {
      PATH: [dirFirst, realLater, "/usr/bin", "/bin"].join(path.delimiter),
      HOME: os.homedir(),
    },
  });
  assert.equal(res.status, 0, `dir on PATH must be skipped: ${res.stderr}`);
  assert.equal(res.stdout.trim(), expectedVersion);
  console.log("J: directory named hardgate skipped -- OK");
}

// --- K. launcherDepth sanitizes tampered depth (fuse input contract) ---
// No mocks: the function reads only process.env, so rows set it directly.
// The "-1" row kills the `&&` -> `||` mutant (it would return -1 and weaken
// the fuse); the "3" rows kill the `||` -> `&&` and `>=` -> `<` mutants,
// which otherwise hang the self-exec test until timeout.
{
  const { createRequire } = await import("node:module");
  const requireK = createRequire(import.meta.url);
  const { launcherDepth } = requireK("../npm/hardgate/bin/hardgate.js");
  const saved = process.env.HARDGATE_LAUNCHER_DEPTH;
  try {
    const cases = [
      [undefined, 0],
      ["0", 0],
      ["3", 3],
      ["-1", 0],
      ["abc", 0],
      ["", 0],
    ];
    for (const [val, expected] of cases) {
      if (val === undefined) delete process.env.HARDGATE_LAUNCHER_DEPTH;
      else process.env.HARDGATE_LAUNCHER_DEPTH = val;
      assert.equal(launcherDepth(), expected, `launcherDepth(${val})`);
    }
  } finally {
    if (saved === undefined) delete process.env.HARDGATE_LAUNCHER_DEPTH;
    else process.env.HARDGATE_LAUNCHER_DEPTH = saved;
  }
  console.log("K: launcherDepth sanitization -- OK");
}

// --- L. spawn contract: stdio, window hiding, depth propagation ---
// Asserts the child-spawn shape without spawning anything, so it holds on
// every platform. Kills the `windowsHide` true->false mutant and the depth
// `+` -> `-` mutant (tampered propagation would silently break the fuse).
{
  const { createRequire } = await import("node:module");
  const requireL = createRequire(import.meta.url);
  const { spawnOptions, launcherDepth: depthOf } = requireL(
    "../npm/hardgate/bin/hardgate.js",
  );
  const saved = process.env.HARDGATE_LAUNCHER_DEPTH;
  try {
    process.env.HARDGATE_LAUNCHER_DEPTH = "3";
    const opts = spawnOptions();
    assert.equal(opts.stdio, "inherit");
    assert.equal(opts.windowsHide, true);
    assert.equal(opts.env.HARDGATE_LAUNCHER_DEPTH, "4");
    assert.equal(opts.env.HARDGATE_LAUNCHER_DEPTH, String(depthOf() + 1));
  } finally {
    if (saved === undefined) delete process.env.HARDGATE_LAUNCHER_DEPTH;
    else process.env.HARDGATE_LAUNCHER_DEPTH = saved;
  }
  console.log("L: spawn contract -- OK");
}

console.log("npm-wrapper-regression.test: OK");
