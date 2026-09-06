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

// G. Resolve the supported platform and reject deferred platforms.
{
  const cases = [
    { platform: "linux", arch: "x64", musl: false, expected: "hardgate-linux-x64" },
    { platform: "linux", arch: "x64", musl: true, expected: null },
    { platform: "linux", arch: "arm64", musl: false, expected: null },
    { platform: "linux", arch: "arm64", musl: true, expected: null },
    { platform: "darwin", arch: "x64", musl: null, expected: null },
    { platform: "darwin", arch: "x64", musl: false, expected: null },
    { platform: "darwin", arch: "arm64", musl: null, expected: null },
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
  console.log("G: supported and rejected platforms -- OK");
}

// H. Require positive glibc evidence without Alpine-specific marker files.
{
  const cases = [
    { platform: "linux", version: "2.39", expected: false },
    { platform: "linux", version: " 2.39 ", expected: false },
    { platform: "linux", version: "", expected: true },
    { platform: "linux", version: "   ", expected: true },
    { platform: "linux", version: null, expected: true },
    { platform: "linux", version: undefined, expected: true },
    { platform: "darwin", version: null, expected: false },
    { platform: "win32", version: null, expected: false },
    { platform: "freebsd", version: null, expected: false },
  ];
  for (const testCase of cases) {
    const { platform, version, expected } = testCase;
    assert.equal(
      launcher.detectMusl(platform, version),
      expected,
      `detectMusl(${platform}, ${version})`,
    );
  }
  console.log("H: musl-detection truth table -- OK");
}

// I. Only ELF magic bytes are accepted as supported machine binaries.
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
    ["macho-le64", Buffer.from([0xcf, 0xfa, 0xed, 0xfe]), false],
    ["macho-fat", Buffer.from([0xca, 0xfe, 0xba, 0xbe]), false],
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
