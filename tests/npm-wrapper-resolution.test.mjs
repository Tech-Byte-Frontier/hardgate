// Unit checks for npm launcher platform, libc, and candidate resolution.
"use strict";
import assert from "node:assert/strict";
import {
  fs,
  loadLauncher,
  makeTempDir,
  path,
  realBinary,
  repoRoot,
} from "./support/npm-wrapper/helpers.mjs";

const root = repoRoot(import.meta.url);
const launcherFile = path.join(root, "npm/hardgate/bin/hardgate.js");
const launcher = loadLauncher(launcherFile);

// G. Resolve every supported platform and libc combination, including the
// Linux fallback package used when optional dependencies were omitted.
{
  const cases = [
    { platform: "linux", arch: "x64", musl: false, expected: "hardgate-linux-x64" },
    { platform: "linux", arch: "x64", musl: true, expected: "hardgate-linux-x64-musl" },
    { platform: "linux", arch: "arm64", musl: false, expected: "hardgate-linux-arm64" },
    { platform: "linux", arch: "arm64", musl: true, expected: "hardgate-linux-arm64-musl" },
    { platform: "darwin", arch: "x64", musl: null, expected: "hardgate-darwin-x64" },
    { platform: "darwin", arch: "x64", musl: false, expected: "hardgate-darwin-x64" },
    { platform: "darwin", arch: "arm64", musl: null, expected: "hardgate-darwin-arm64" },
    { platform: "win32", arch: "x64", musl: null, expected: null },
    { platform: "win32", arch: "arm64", musl: null, expected: null },
    { platform: "linux", arch: "s390x", musl: false, expected: null },
    { platform: "freebsd", arch: "x64", musl: null, expected: null },
  ];
  for (const testCase of cases) {
    const { platform, arch, musl, expected } = testCase;
    assert.equal(
      launcher.resolvePlatform(platform, arch, musl),
      expected,
      `${platform}/${arch} musl=${musl}`,
    );
  }
  assert.deepEqual(launcher.fallbackPackages("hardgate-linux-x64"), [
    "hardgate-linux-x64-musl",
  ]);
  assert.deepEqual(launcher.fallbackPackages("hardgate-linux-x64-musl"), [
    "hardgate-linux-x64",
  ]);
  assert.deepEqual(launcher.fallbackPackages("hardgate-linux-arm64"), [
    "hardgate-linux-arm64-musl",
  ]);
  assert.deepEqual(launcher.fallbackPackages("hardgate-darwin-arm64"), []);
  console.log("G: platform matrix + musl fallbacks pinned -- OK");
}

// H. glibc evidence wins; otherwise Alpine's release marker identifies musl.
{
  const cases = [
    { platform: "linux", version: "2.39", alpine: false, expected: false },
    { platform: "linux", version: "2.39", alpine: true, expected: false },
    { platform: "linux", version: null, alpine: true, expected: true },
    { platform: "linux", version: null, alpine: false, expected: false },
    { platform: "linux", version: undefined, alpine: true, expected: true },
    { platform: "darwin", version: null, alpine: false, expected: false },
    { platform: "darwin", version: null, alpine: true, expected: false },
    { platform: "win32", version: null, alpine: true, expected: false },
    { platform: "freebsd", version: null, alpine: false, expected: false },
  ];
  for (const testCase of cases) {
    const { platform, version, alpine, expected } = testCase;
    assert.equal(
      launcher.detectMusl(platform, version, alpine),
      expected,
      `detectMusl(${platform}, ${version}, ${alpine})`,
    );
  }
  console.log("H: musl-detection truth table -- OK");
}

// I. Only ELF and Mach-O magic bytes are accepted as machine binaries.
{
  const dir = makeTempDir("hg-magic-");
  const files = [
    ["elf", Buffer.from([0x7f, 0x45, 0x4c, 0x46]), true],
    ["elf-truncated", Buffer.from([0x7f, 0x45]), false],
    ["elf-lookalike", Buffer.from([0x7f, 0x4f, 0x4f, 0x4f]), false],
    ["elf-first-two", Buffer.from([0x7f, 0x45, 0x00, 0x00]), false],
    ["elf-first-three", Buffer.from([0x7f, 0x45, 0x4c, 0x00]), false],
    ["pe", Buffer.from([0x4d, 0x5a, 0x90, 0x00]), false],
    ["pe-lookalike", Buffer.from([0x4d, 0x00, 0x00, 0x00]), false],
    ["macho-le64", Buffer.from([0xcf, 0xfa, 0xed, 0xfe]), true],
    ["macho-fat", Buffer.from([0xca, 0xfe, 0xba, 0xbe]), true],
    ["shell", Buffer.from("#!/bin/sh\necho hi\n"), false],
    ["empty", Buffer.alloc(0), false],
    ["text", Buffer.from("not a binary"), false],
  ];
  for (const [name, bytes, expected] of files) {
    const file = path.join(dir, name);
    fs.writeFileSync(file, bytes);
    assert.equal(launcher.isRealBinary(file), expected, `isRealBinary(${name})`);
  }
  assert.equal(launcher.isRealBinary(path.join(dir, "missing")), false);
  console.log("I: binary magic-byte gate -- OK");
}

// I2. Self-recognition is symlink-aware and tolerates broken links.
{
  const dir = makeTempDir("hg-self-");
  assert.equal(launcher.isSelf(launcherFile), true);
  assert.equal(launcher.isSelf(realBinary(root)), false);
  const dangling = path.join(dir, "dangling");
  fs.symlinkSync(path.join(dir, "no-such-target"), dangling);
  assert.equal(launcher.isSelf(dangling), false);
  assert.equal(launcher.isSelf(path.join(dir, "missing")), false);
  console.log("I2: self-recognition + broken-link tolerance -- OK");
}
