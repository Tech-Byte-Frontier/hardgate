#!/usr/bin/env node
// hardgate npm launcher.
// Resolves the prebuilt Rust binary from the platform-specific optional
// dependency (pnpm / npm / yarn / bun compatible) and execs it.
// No dependencies, no network. Set HARDGATE_BINARY to override.
"use strict";

const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

function detectMusl(platform, glibcVersionRuntime, alpineReleaseExists) {
  if (platform !== "linux") return false;
  if (typeof glibcVersionRuntime === "string") return false;
  return alpineReleaseExists;
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

function hasAlpineRelease() {
  try {
    return fs.existsSync("/etc/alpine-release");
  } catch {
    return false;
  }
}

function isMusl() {
  if (process.platform !== "linux") return false;
  return detectMusl(
    process.platform,
    readGlibcVersion(),
    hasAlpineRelease(),
  );
}

// Platform -> optional-dependency table: [platform, arch, musl-or-null, pkg].
// `null` musl means "any libc". Table-driven so complexity budgets stay flat
// as new targets are added.
const PLATFORM_TABLE = [
  ["linux", "x64", false, "hardgate-linux-x64"],
  ["linux", "x64", true, "hardgate-linux-x64-musl"],
  ["linux", "arm64", false, "hardgate-linux-arm64"],
  ["linux", "arm64", true, "hardgate-linux-arm64-musl"],
  ["darwin", "x64", null, "hardgate-darwin-x64"],
  ["darwin", "arm64", null, "hardgate-darwin-arm64"],
  ["win32", "x64", null, "hardgate-win32-x64"],
];

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

// On Linux the musl (static) binary also runs on glibc hosts, so it is a
// valid fallback when the preferred optional dep was skipped
// (e.g. installed with --no-optional / --omit=optional).
function fallbackPackages(primary) {
  if (primary === "hardgate-linux-x64") return ["hardgate-linux-x64-musl"];
  if (primary === "hardgate-linux-x64-musl") return ["hardgate-linux-x64"];
  if (primary === "hardgate-linux-arm64") return ["hardgate-linux-arm64-musl"];
  if (primary === "hardgate-linux-arm64-musl") return ["hardgate-linux-arm64"];
  return [];
}

function binaryName(pkg) {
  return pkg === "hardgate-win32-x64" ? "hardgate.exe" : "hardgate";
}

// Guard against wrapper scripts: only accept real machine binaries
// (ELF / Mach-O / PE). The PATH fallback must never resolve to another
// `hardgate` launcher shim (e.g. an npm/pnpm `.bin` entry): exec'ing it
// would re-enter this launcher with the same argv and recurse until the
// kernel refuses with E2BIG. Seen in the wild via `pnpm dlx` sandboxes
// where `.bin` is first on PATH and the platform package is absent.
// Mach-O magic numbers (BE + LE spellings) and FAT binaries.
const MACHO_U32 = new Set([0xfeedface, 0xfeedfacf, 0xcafebabe]);

function magicMatches(buf) {
  const isElf =
    buf[0] === 0x7f && buf[1] === 0x45 && buf[2] === 0x4c && buf[3] === 0x46;
  if (isElf) return true;
  if (buf[0] === 0x4d && buf[1] === 0x5a) return true; // MZ (windows PE)
  return MACHO_U32.has(buf.readUInt32LE(0)) || MACHO_U32.has(buf.readUInt32BE(0));
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
  const bin = binaryName(pkg);
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
  const suffix = process.platform === "win32" ? ".exe" : "";
  const rels = [
    ["..", "..", "..", "target", "release", `hardgate${suffix}`],
    ["..", "..", "..", "target", "debug", `hardgate${suffix}`],
  ];
  for (const rel of rels) {
    const candidate = path.join(__dirname, ...rel);
    if (acceptCandidate(candidate)) return candidate;
  }
  return null;
}

function resolvePathBinary() {
  const suffix = process.platform === "win32" ? ".exe" : "";
  const dirs = (process.env.PATH || "").split(path.delimiter);
  for (const dir of dirs) {
    if (!dir) continue;
    const candidate = path.join(dir, `hardgate${suffix}`);
    if (acceptCandidate(candidate)) return candidate;
  }
  return null;
}

function findBinary() {
  if (process.env.HARDGATE_BINARY) return process.env.HARDGATE_BINARY;

  const primary = platformPackage();
  const candidates = primary
    ? [primary, ...fallbackPackages(primary)]
    : [];
  for (const pkg of candidates) {
    const found = tryResolve(pkg);
    if (found) return found;
  }

  // 3. Rust workspace dev fallback (running from the hardgate repo itself).
  // 4. System PATH (cargo install / curl installer / brew).
  return resolveDevBinary() ?? resolvePathBinary();
}

// Recursion fuse: spawning `bin` below re-enters a launcher when `bin` is
// itself a wrapper. The acceptCandidate guards should prevent that, but a
// hard stop guarantees a clear error instead of unbounded nesting.
function launcherDepth() {
  const n = Number.parseInt(process.env.HARDGATE_LAUNCHER_DEPTH || "0", 10);
  return Number.isFinite(n) && n >= 0 ? n : 0;
}

function main() {
  if (launcherDepth() > 5) {
    console.error(
      "[hardgate] Refusing to recurse: resolved binary re-entered the npm launcher. " +
        "Set HARDGATE_BINARY to the real binary or reinstall the platform package.",
    );
    process.exit(1);
  }
  const bin = findBinary();
  if (!bin) {
    const primary = platformPackage() || "<unknown-platform>";
    console.error(
      `[hardgate] No prebuilt binary found for ${process.platform}/${process.arch} (expected optional dep '${primary}').`,
    );
    console.error(
      "[hardgate] Fix: reinstall without --no-optional/--omit=optional, or install the Rust toolchain fallback with `cargo install hardgate`, or download a tarball from https://github.com/Tech-Byte-Frontier/hardgate/releases",
    );
    process.exit(1);
  }
  const args = process.argv.slice(2);
  const res = spawnSync(bin, args, {
    stdio: "inherit",
    windowsHide: true,
    env: {
      ...process.env,
      HARDGATE_LAUNCHER_DEPTH: String(launcherDepth() + 1),
    },
  });
  if (res.error) {
    if (res.error.code === "ENOENT") {
      console.error(`[hardgate] Binary not executable: ${bin}`);
      process.exit(1);
    }
    throw res.error;
  }
  process.exit(res.status ?? 0);
}

if (require.main === module) main();
module.exports = {
  platformPackage,
  resolvePlatform,
  detectMusl,
  fallbackPackages,
  findBinary,
  isRealBinary,
  isSelf,
  launcherDepth,
};
