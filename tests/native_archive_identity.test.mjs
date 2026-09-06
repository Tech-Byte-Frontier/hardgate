"use strict";

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { verifyNativeArchive } from "../scripts/native-channel-archive.mjs";

const directory = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-archive-identity-test-"));
const packageName = "hardgate-linux-x64";
const version = "0.6.0";
const sourceSha = "1234567890abcdef1234567890abcdef12345678";
const target = "x86_64-unknown-linux-gnu";

function run(command, args) {
  const result = spawnSync(command, args, { encoding: "utf8", timeout: 30_000 });
  assert.equal(result.status, 0, result.stderr || result.error?.message);
  return result.stdout;
}

function archiveWithIdentity(identity, targetMarker) {
  const root = path.join(directory, packageName);
  fs.mkdirSync(root, { recursive: true });
  const source = path.join(directory, "fixture.c");
  fs.writeFileSync(source, `#include <stdio.h>
const char version_display[] = "${identity}";
const char target_display[] = "hardgate-target:${targetMarker}";
int main(void) { printf("hardgate %s\\n", version_display); return 0; }
`);
  run("/usr/bin/cc", [source, "-o", path.join(root, "hardgate")]);
  fs.writeFileSync(path.join(root, "BUILD-METADATA.json"), JSON.stringify({
    name: "hardgate", version, target, package: packageName, commit: sourceSha,
  }));
  const archive = path.join(directory, `${packageName}.tar.gz`);
  run("/usr/bin/tar", ["-czf", archive, "-C", directory, packageName]);
  return { archive, packageName, version, sourceSha, directory };
}

try {
  const options = archiveWithIdentity(`${version} (${sourceSha})`, target);
  const binary = path.join(directory, packageName, "hardgate");
  assert.equal(fs.readFileSync(binary).includes(Buffer.from(`hardgate ${version} (${sourceSha})`)), false);
  assert.equal(run(binary, ["--version"]).trim(), `hardgate ${version} (${sourceSha})`);
  assert.match((await verifyNativeArchive(options)).sha256, /^[a-f0-9]{64}$/);
  await assert.rejects(
    verifyNativeArchive(archiveWithIdentity(`${version} separate ${sourceSha}`, target)),
    /expected version\/source identity marker/,
  );
  await assert.rejects(
    verifyNativeArchive(archiveWithIdentity(`${version} (${sourceSha})`, "wrong-target")),
    /expected Cargo target marker/,
  );
} finally {
  fs.rmSync(directory, { recursive: true, force: true });
}

console.log("native_archive_identity.test: OK");
