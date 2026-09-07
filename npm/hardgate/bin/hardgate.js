#!/usr/bin/env node
// hardgate npm launcher.
// Resolves the prebuilt Rust binary from the platform-specific optional
// dependency (npm / pnpm compatible) and execs it.
// No dependencies, no network. Set HARDGATE_BINARY to override.
"use strict";

const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

function detectMusl(platform, glibcVersionRuntime) {
  if (platform !== "linux") return false;
  return !(
    typeof glibcVersionRuntime === "string" &&
    glibcVersionRuntime.trim().length > 0
  );
}

function readGlibcVersion() {
  try {
    const report =
      typeof process.report?.getReport === "function"
        ? process.report.getReport()
        : null;
    return report?.header?.glibcVersionRuntime ?? null;
  } catch {
    return null;
  }
}

function isMusl() {
  // Require positive glibc evidence; unknown Linux libc is unsupported.
  return detectMusl(process.platform, readGlibcVersion());
}

const PLATFORM_TABLE = [
  ["linux", "x64", false, "hardgate-linux-x64"],
  ["linux", "arm64", false, "hardgate-linux-arm64"],
  ["darwin", "x64", null, "hardgate-darwin-x64"],
  ["darwin", "arm64", null, "hardgate-darwin-arm64"],
  ["win32", "x64", null, "hardgate-win32-x64"],
];
const BINARY_NAME = process.platform === "win32" ? "hardgate.exe" : "hardgate";

function resolvePlatform(platform, arch, musl) {
  const hit = PLATFORM_TABLE.find(
    ([plat, wantArch, wantMusl]) =>
      plat === platform &&
      wantArch === arch &&
      (wantMusl === null || wantMusl === musl),
  );
  return hit ? hit[3] : null;
}

function platformPackage() {
  const musl = process.platform === "linux" ? isMusl() : null;
  return resolvePlatform(process.platform, process.arch, musl);
}

// Accept native executable headers for this host. Reject launcher shims to avoid recursion
// through an npm or pnpm .bin entry on PATH.
function magicMatches(buf) {
  if (process.platform === "win32") return buf[0] === 0x4d && buf[1] === 0x5a;
  if (process.platform === "darwin") {
    return [0xfeedfacf, 0xcffaedfe, 0xcafebabe, 0xbebafeca, 0xcafebabf, 0xbfbafeca].includes(buf.readUInt32BE(0));
  }
  return buf[0] === 0x7f && buf[1] === 0x45 && buf[2] === 0x4c && buf[3] === 0x46;
}

function readMagic(p) {
  const fd = fs.openSync(p, "r");
  try {
    const buf = Buffer.alloc(4);
    return fs.readSync(fd, buf, 0, 4, 0) === 4 ? buf : null;
  } finally {
    try {
      fs.closeSync(fd);
    } catch {
      /* ignore */
    }
  }
}

function isRealBinary(p) {
  try {
    const buf = readMagic(p);
    return buf !== null && magicMatches(buf);
  } catch {
    return false;
  }
}

let selfRealPath = null;
function isSelf(p) {
  try {
    selfRealPath ??= fs.realpathSync(__filename);
    return fs.realpathSync(p) === selfRealPath;
  } catch {
    return false;
  }
}

function acceptCandidate(p) {
  try {
    if (!fs.existsSync(p) || isSelf(p)) return false;
    const st = fs.statSync(p);
    if (!st.isFile()) return false;
    return isRealBinary(p);
  } catch {
    return false;
  }
}

function pkgBinary(pkgDir, bin) {
  return path.join(pkgDir, "bin", bin);
}

function resolveViaNode(pkg, bin) {
  try {
    const dir = path.dirname(require.resolve(`${pkg}/package.json`));
    const candidate = pkgBinary(dir, bin);
    if (acceptCandidate(candidate)) return candidate;
  } catch {
    /* fall through to explicit layout probes */
  }
  return null;
}

function resolveViaPaths(pkg, bin) {
  const fromDirs = [path.join(__dirname, "..", ".."), __dirname];
  for (const from of fromDirs) {
    try {
      const req = require.resolve(`${pkg}/package.json`, { paths: [from] });
      const candidate = pkgBinary(path.dirname(req), bin);
      if (acceptCandidate(candidate)) return candidate;
    } catch {
      /* not visible from here -- try next */
    }
  }
  return null;
}

function resolveViaSiblings(pkg, bin) {
  const bases = [
    path.join(__dirname, "..", pkg),
    path.join(__dirname, "..", "..", pkg),
  ];
  for (const base of bases) {
    if (acceptCandidate(pkgBinary(base, bin))) return pkgBinary(base, bin);
  }
  return null;
}

function tryResolve(pkg) {
  const bin = BINARY_NAME;
  // NOTE: there is deliberately no argv[1]-based lookup. Node resolves the
  // entry-point path (symlinks + `..`) before user code runs, so argv[1]
  // always shows the content-addressed store path under pnpm -- never the
  // logical consumer tree. Registry installs are covered by 1a (pnpm links
  // optional deps as `.pnpm` siblings); the rest by 1b/2/3/4.
  return (
    resolveViaNode(pkg, bin) ??
    resolveViaPaths(pkg, bin) ??
    resolveViaSiblings(pkg, bin)
  );
}

function resolveDevBinary() {
  const rels = [
    ["..", "..", "..", "target", "release", BINARY_NAME],
    ["..", "..", "..", "target", "debug", BINARY_NAME],
  ];
  for (const rel of rels) {
    const candidate = path.join(__dirname, ...rel);
    if (acceptCandidate(candidate)) return candidate;
  }
  return null;
}

function resolvePathBinary() {
  const dirs = (process.env.PATH || "").split(path.delimiter);
  for (const dir of dirs) {
    if (!dir) continue;
    const candidate = path.join(dir, BINARY_NAME);
    if (acceptCandidate(candidate)) return candidate;
  }
  return null;
}

function findBinary() {
  const primary = platformPackage();
  if (!primary) return null;
  if (process.env.HARDGATE_BINARY) return process.env.HARDGATE_BINARY;
  const found = tryResolve(primary);
  if (found) return found;

  // 3. Rust workspace dev fallback (running from the hardgate repo itself).
  // 4. System PATH (cargo install or a direct release download).
  return resolveDevBinary() ?? resolvePathBinary();
}

// Recursion fuse: spawning `bin` below re-enters a launcher when `bin` is
// itself a wrapper. The acceptCandidate guards should prevent that, but a
// hard stop guarantees a clear error instead of unbounded nesting.
function launcherDepth() {
  const n = Number.parseInt(process.env.HARDGATE_LAUNCHER_DEPTH || "0", 10);
  return Number.isFinite(n) && n >= 0 ? n : 0;
}

// Spawn contract in one place so stdio inheritance, Windows window hiding,
// and fuse depth propagation are unit-assertable without spawning anything.
function spawnOptions() {
  return {
    stdio: "inherit",
    windowsHide: true,
    env: {
      ...process.env,
      HARDGATE_LAUNCHER_DEPTH: String(launcherDepth() + 1),
    },
  };
}

function reportFailure(...messages) {
  for (const message of messages) {
    fs.writeSync(2, `${message}\n`);
  }
  process.exitCode = 1;
}

function exitFromSpawn(result) {
  if (!result.signal) {
    process.exit(result.status ?? 1);
    return;
  }
  if (process.platform !== "win32") {
    try {
      process.kill(process.pid, result.signal);
      return;
    } catch {
      /* fall through to a deterministic nonzero exit */
    }
  }
  process.exit(1);
}

function main() {
  if (launcherDepth() > 5) {
    reportFailure(
      "[hardgate] Refusing to recurse: resolved binary re-entered the npm launcher. " +
        "Set HARDGATE_BINARY to the real binary or reinstall the platform package.",
    );
    return;
  }
  const primary = platformPackage();
  if (!primary) {
    reportFailure(
      `[hardgate] Unsupported platform ${process.platform}/${process.arch}; prebuilt binaries support Linux glibc and macOS (x64/arm64), and Windows x64. Try a source build with cargo install hardgate --locked for other targets.`,
    );
    return;
  }
  const bin = findBinary();
  if (!bin) {
    const expected = primary || "<unknown-platform>";
    reportFailure(
      `[hardgate] No prebuilt binary found for ${process.platform}/${process.arch} (expected optional dep '${expected}').`,
      "[hardgate] Fix: reinstall without --no-optional/--omit=optional, or install the Rust toolchain fallback with `cargo install hardgate`, or download a tarball from https://github.com/Tech-Byte-Frontier/hardgate/releases",
    );
    return;
  }
  const args = process.argv.slice(2);
  const res = spawnSync(bin, args, spawnOptions());
  if (res.error) {
    if (res.error.code === "ENOENT") {
      reportFailure(`[hardgate] Binary not executable: ${bin}`);
      return;
    }
    throw res.error;
  }
  exitFromSpawn(res);
}

if (require.main === module) main();
module.exports = {
  platformPackage,
  resolvePlatform,
  detectMusl,
  findBinary,
  isRealBinary,
  isSelf,
  launcherDepth,
  spawnOptions,
  exitFromSpawn,
};
