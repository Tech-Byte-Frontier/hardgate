// Offline GNU ABI contract: missing or conflicting evidence is blocking.
"use strict";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { classifyBinaryAbi } from "../scripts/release-abi.mjs";

const fixture = { report: "ELF 64-bit LSB pie executable, x86-64, dynamically linked", programHeaders: "[Requesting program interpreter: /lib64/ld-linux-x86-64.so.2]", symbols: "", abi: "gnu" };
assert.equal(classifyBinaryAbi(fixture).ok, true);
assert.equal(classifyBinaryAbi({ ...fixture, programHeaders: "" }).ok, false);
assert.equal(classifyBinaryAbi({ ...fixture, report: "Mach-O 64-bit" }).ok, false);
assert.equal(classifyBinaryAbi({ ...fixture, programHeaders: "/lib/ld-musl-x86_64.so.1" }).ok, false);
for (const abi of ["musl", "msvc", null, "unknown"]) assert.equal(classifyBinaryAbi({ ...fixture, abi }).ok, false);
assert.equal(classifyBinaryAbi({ ...fixture, symbols: "__libc_start_main@GLIBC_2.39" }).ok, true);
assert.equal(classifyBinaryAbi({ ...fixture, symbols: "future@GLIBC_2.40" }).ok, false);
const inspect = (command, args) => {
  const result = spawnSync(command, args, { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout;
};
if (process.platform === "linux") assert.equal(classifyBinaryAbi({
  report: inspect("file", ["-b", "/usr/bin/true"]),
  programHeaders: inspect("readelf", ["-l", "/usr/bin/true"]),
  symbols: inspect("readelf", ["-sW", "/usr/bin/true"]),
  abi: "gnu",
}).ok, true, "the real GNU system executable must supply positive ABI evidence");
console.log("release_contract.abi: GNU evidence and unsupported ABI rejection OK");

for (const [abi, report] of [["darwin", "Mach-O 64-bit arm64 executable"]]) {
  assert.equal(classifyBinaryAbi({ abi, report }).ok, true);
  assert.equal(classifyBinaryAbi({ abi, report: "ELF 64-bit" }).ok, false);
  assert.equal(classifyBinaryAbi({ abi, report: "ASCII text" }).ok, false);
}
