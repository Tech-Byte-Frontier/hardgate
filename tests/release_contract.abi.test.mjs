// Offline ABI contract: a static ELF must carry the exact Cargo target marker
// and must not expose glibc evidence. Dynamic musl binaries additionally need
// their musl interpreter; symbols are defense-in-depth evidence.
"use strict";

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { classifyBinaryAbi } from "../scripts/release-abi.mjs";

const staticGlibcFixture = {
  report: "ELF 64-bit LSB executable, x86-64, statically linked",
  programHeaders: "",
  symbols: "",
  abi: "musl",
  targetMarkerValid: false,
};
const rejected = classifyBinaryAbi(staticGlibcFixture);
assert.equal(rejected.ok, false, "static glibc fixture must fail musl verification");
assert.match(rejected.reason, /exact Cargo target marker/);

const staticMuslMarker = classifyBinaryAbi({
  ...staticGlibcFixture,
  targetMarkerValid: true,
});
assert.equal(staticMuslMarker.ok, true, "an exact Cargo target marker proves a stripped static musl build");

// A copied Cargo target marker cannot make a stripped static glibc executable
// pass. Build the adversarial ELF with the runner's system compiler so the
// contract exercises real file/readelf evidence, including glibc's retained
// .note.ABI-tag, rather than a hand-written report that could drift.
if (process.platform === "linux") {
  const fixtureDirectory = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-abi-contract-"));
  try {
    const source = path.join(fixtureDirectory, "fixture.c");
    const binary = path.join(fixtureDirectory, "fixture");
    fs.writeFileSync(source, "int main(void) { return 0; }\n");
    const compiled = spawnSync("cc", ["-static", "-s", "-O2", source, "-o", binary], { encoding: "utf8" });
    assert.equal(compiled.status, 0, `cc -static fixture failed: ${compiled.error?.message ?? compiled.stderr}`);

    const run = (command, args) => {
      const result = spawnSync(command, args, { encoding: "utf8" });
      assert.equal(result.status, 0, `${command} fixture inspection failed: ${result.error?.message ?? result.stderr}`);
      return result.stdout;
    };
    const report = run("file", ["-b", binary]);
    const programHeaders = run("readelf", ["-l", binary]);
    const symbols = run("readelf", ["-sW", binary]);
    const notes = run("readelf", ["-n", binary]);
    assert.match(report, /statically linked/i, "fixture must be statically linked");
    assert.match(notes, /(?:\.note\.ABI-tag|NT_GNU_ABI_TAG|GNU ABI tag)/i, "fixture must retain glibc ABI note");

    const copiedTargetMarker = Buffer.from("hardgate-target:x86_64-unknown-linux-musl", "utf8");
    fs.appendFileSync(binary, Buffer.concat([Buffer.from("\0", "utf8"), copiedTargetMarker]));
    assert.ok(fs.readFileSync(binary).includes(copiedTargetMarker), "fixture must contain the copied target marker");
    const forged = classifyBinaryAbi({
      report,
      programHeaders,
      symbols,
      notes,
      abi: "musl",
      targetMarkerValid: true,
    });
    assert.equal(forged.ok, false, "stripped static glibc with copied target marker must fail musl verification");
    assert.match(forged.reason, /glibc markers|positive __init_libc/i);
  } finally {
    fs.rmSync(fixtureDirectory, { recursive: true, force: true });
  }
}

const staticMuslSymbol = classifyBinaryAbi({
  ...staticGlibcFixture,
  symbols: "  42: 0000000000000000     0 FUNC    GLOBAL DEFAULT  UND __init_libc",
  targetMarkerValid: true,
});
assert.equal(staticMuslSymbol.ok, true, "the exact Cargo marker is positive static musl evidence");

const dynamicMusl = classifyBinaryAbi({
  report: "ELF 64-bit LSB pie executable, dynamically linked",
  programHeaders: "[Requesting program interpreter: /lib/ld-musl-x86_64.so.1]",
  symbols: "",
  abi: "musl",
  targetMarkerValid: false,
});
assert.equal(dynamicMusl.ok, true, "musl interpreter is positive dynamic musl evidence");

const glibcMarkers = classifyBinaryAbi({
  ...staticGlibcFixture,
  targetMarkerValid: true,
  symbols: "  1: 0000000000000000 FUNC GLOBAL DEFAULT UND __libc_start_main@GLIBC_2.34",
});
assert.equal(glibcMarkers.ok, false, "glibc symbols must be rejected even with a target marker");
assert.match(glibcMarkers.reason, /glibc markers/);

console.log("release_contract.abi: positive musl evidence and static-glibc rejection OK");
