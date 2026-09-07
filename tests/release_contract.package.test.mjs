// Regression for reproducible archive modes: release-package must emit the
// same bytes when invoked under restrictive and permissive caller umasks.
"use strict";

import { NATIVE_PACKAGES, executableName } from "../scripts/release-platforms.mjs";
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const packageScript = path.join(root, "scripts/release-package.mjs");
const fixture = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-package-contract-"));
const incoming = path.join(fixture, "incoming");
const output022 = path.join(fixture, "output-022");
const output077 = path.join(fixture, "output-077");
const version = "9.9.9";
const commit = "0123456789abcdef0123456789abcdef01234567";
const targets = Object.values(NATIVE_PACKAGES).map(({target, name}) => [target, name]);

function digest(file) {
  return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}

function runWithUmask(mask, output) {
  const args = [
    "--incoming",
    incoming,
    "--output",
    output,
    "--version",
    version,
    "--commit",
    commit,
  ];
  return spawnSync(
    "sh",
    ["-c", 'umask "$1"; shift; exec "$@"', "release-package-umask", mask, process.execPath, packageScript, ...args],
    { cwd: root, encoding: "utf8", maxBuffer: 1024 * 1024 },
  );
}

try {
  for (const [target, packageName] of targets) {
    const binaryDirectory = path.join(incoming, `binary-${target}-attempt-10`);
    const staleDirectory = path.join(incoming, `binary-${target}-attempt-2`);
    fs.mkdirSync(staleDirectory, { recursive: true });
    fs.writeFileSync(path.join(staleDirectory, executableName(packageName)), "stale attempt\n");
    fs.mkdirSync(binaryDirectory, { recursive: true });
    fs.writeFileSync(path.join(binaryDirectory, executableName(packageName)), `fixture ${target}\n`, { mode: 0o755 });
  }

  const permissive = runWithUmask("022", output022);
  assert.equal(permissive.status, 0, `umask 022 package failed: ${permissive.stderr}`);
  const restrictive = runWithUmask("077", output077);
  assert.equal(restrictive.status, 0, `umask 077 package failed: ${restrictive.stderr}`);

  const checksums022 = fs.readFileSync(path.join(output022, "SHA256SUMS"), "utf8");
  const checksums077 = fs.readFileSync(path.join(output077, "SHA256SUMS"), "utf8");
  assert.equal(checksums077, checksums022, "SHA256SUMS must be stable across caller umasks");
  for (const [target, packageName] of targets) {
    const archive022 = path.join(output022, `${packageName}.tar.gz`);
    const archive077 = path.join(output077, `${packageName}.tar.gz`);
    assert.equal(digest(archive077), digest(archive022), `${packageName} archive changed with umask`);
    const extracted = spawnSync("tar", ["-xOzf", archive022, `${packageName}/${executableName(packageName)}`], { encoding: "utf8" });
    assert.equal(extracted.status, 0, extracted.stderr);
    assert.equal(extracted.stdout, `fixture ${target}\n`, "latest numeric attempt must supply the binary");
    const listing = spawnSync("tar", ["-tvzf", archive022], { encoding: "utf8" });
    assert.equal(listing.status, 0, `cannot inspect ${packageName}: ${listing.stderr}`);
    assert.match(listing.stdout, new RegExp(`^drwxr-xr-x .* ${packageName}/$`, "m"));
    assert.match(listing.stdout, new RegExp(`^-rwxr-xr-x .* ${packageName}/${executableName(packageName).replace(".", "\\.")}$`, "m"));
    assert.match(listing.stdout, new RegExp(`^-rw-r--r-- .* ${packageName}/BUILD-METADATA\\.json$`, "m"));
  }
  console.log("release_contract.package: archive bytes and modes stable across umasks");
} finally {
  fs.rmSync(fixture, { recursive: true, force: true });
}
