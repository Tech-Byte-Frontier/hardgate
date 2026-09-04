// Runtime contract for the POSIX installer checksum path. The fake
// sha256sum intentionally rejects GNU-only verification flags so this test
// exercises the BusyBox-compatible digest invocation used by install.sh.
"use strict";

import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const installer = path.join(root, "scripts/install.sh");
const fixture = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-installer-contract-"));
const fakeBin = path.join(fixture, "bin");
const packageName = "hardgate-linux-x64-musl";
const version = "9.9.9";
const commit = "0123456789abcdef0123456789abcdef01234567";
const expected = `hardgate ${version} (${commit})`;

function writeExecutable(file, source) {
  fs.writeFileSync(file, source, { mode: 0o755 });
  fs.chmodSync(file, 0o755);
}

try {
  fs.mkdirSync(fakeBin, { recursive: true });
  const packageRoot = path.join(fixture, packageName);
  fs.mkdirSync(packageRoot, { recursive: true });
  writeExecutable(
    path.join(packageRoot, "hardgate"),
    `#!/bin/sh\nif [ "$1" = "--version" ]; then printf '%s\\n' '${expected}'; else exit 2; fi\n`,
  );
  fs.writeFileSync(path.join(packageRoot, "BUILD-METADATA.json"), `${JSON.stringify({ name: "hardgate", version, package: packageName, target: "x86_64-unknown-linux-musl", commit })}\n`);
  const archive = path.join(fixture, `${packageName}.tar.gz`);
  const packed = spawnSync("tar", ["-czf", archive, "-C", fixture, packageName], { encoding: "utf8" });
  if (packed.status !== 0) throw new Error(`tar fixture failed: ${packed.stderr}`);
  const checksum = crypto.createHash("sha256").update(fs.readFileSync(archive)).digest("hex");
  fs.writeFileSync(path.join(fixture, "SHA256SUMS"), `${checksum}  ${packageName}.tar.gz\n`);

  writeExecutable(
    path.join(fakeBin, "uname"),
    `#!/bin/sh\ncase "$1" in -s) echo Linux ;; -m) echo x86_64 ;; *) exit 1 ;; esac\n`,
  );
  writeExecutable(path.join(fakeBin, "getconf"), "#!/bin/sh\nexit 1\n");
  writeExecutable(path.join(fakeBin, "ldd"), "#!/bin/sh\nprintf '%s\\n' 'musl libc (x86_64)'\n");
  writeExecutable(
    path.join(fakeBin, "curl"),
    `#!/bin/sh\nset -eu\nout=""; url=""\nwhile [ "$#" -gt 0 ]; do case "$1" in -o) out="$2"; shift ;; http*) url="$1" ;; esac; shift; done\ncase "$url" in *SHA256SUMS) cp "$HARDGATE_FIXTURE/SHA256SUMS" "$out" ;; *.tar.gz) cp "$HARDGATE_FIXTURE/${packageName}.tar.gz" "$out" ;; *) exit 1 ;; esac\n`,
  );
  writeExecutable(
    path.join(fakeBin, "sha256sum"),
    `#!/bin/sh\nfor arg in "$@"; do case "$arg" in --*) echo 'BusyBox shim rejects GNU flags' >&2; exit 64 ;; esac; done\nexec /usr/bin/sha256sum "$@"\n`,
  );

  const installDir = path.join(fixture, "install");
  const runInstaller = (destination) => {
    const environment = {
      ...process.env,
      PATH: `${fakeBin}:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin`,
      HARDGATE_FIXTURE: fixture,
      HARDGATE_VERSION: `v${version}`,
      HARDGATE_INSTALL_DIR: destination,
    };
    delete environment.HOME;
    return spawnSync("sh", [installer], { cwd: root, encoding: "utf8", env: environment });
  };
  const result = runInstaller(installDir);
  if (result.status !== 0) throw new Error(`installer failed under BusyBox checksum shim:\n${result.stdout}${result.stderr}`);
  const installed = path.join(installDir, "hardgate");
  if (!fs.existsSync(installed) || (fs.statSync(installed).mode & 0o111) === 0) {
    throw new Error("installer did not create an executable binary");
  }
  const smoke = spawnSync(installed, ["--version"], { encoding: "utf8" });
  if (smoke.status !== 0 || smoke.stdout.trim() !== expected) {
    throw new Error(`installed fixture identity mismatch: ${smoke.stdout}${smoke.stderr}`);
  }

  const goodArchive = fs.readFileSync(archive);
  const goodChecksums = fs.readFileSync(path.join(fixture, "SHA256SUMS"));
  const writeChecksum = (bytes) => {
    const value = crypto.createHash("sha256").update(bytes).digest("hex");
    fs.writeFileSync(path.join(fixture, "SHA256SUMS"), `${value}  ${packageName}.tar.gz\n`);
  };
  const extraFixtureRoot = path.join(fixture, "extra-fixture");
  const extraPackageRoot = path.join(extraFixtureRoot, packageName);
  fs.mkdirSync(extraFixtureRoot, { recursive: true });
  fs.cpSync(packageRoot, extraPackageRoot, { recursive: true });
  fs.writeFileSync(path.join(extraPackageRoot, "EXTRA.txt"), "unexpected archive member\n");
  const extraArchive = path.join(fixture, "extra.tar.gz");
  const extraPacked = spawnSync("tar", ["-czf", extraArchive, "-C", extraFixtureRoot, packageName], { encoding: "utf8" });
  if (extraPacked.status !== 0) throw new Error(`extra member fixture failed: ${extraPacked.stderr}`);
  fs.copyFileSync(extraArchive, archive);
  writeChecksum(fs.readFileSync(archive));
  const extraMember = runInstaller(path.join(fixture, "extra-member"));
  if (extraMember.status === 0 || !extraMember.stderr.includes("archive members")) {
    throw new Error(`unexpected archive member unexpectedly passed:\n${extraMember.stdout}${extraMember.stderr}`);
  }
  fs.writeFileSync(archive, goodArchive);
  fs.writeFileSync(path.join(fixture, "SHA256SUMS"), goodChecksums);
  const badFixtureRoot = path.join(fixture, "bad-fixture");
  const badPackageRoot = path.join(badFixtureRoot, packageName);
  fs.mkdirSync(badFixtureRoot, { recursive: true });
  fs.cpSync(packageRoot, badPackageRoot, { recursive: true });
  fs.writeFileSync(path.join(badPackageRoot, "BUILD-METADATA.json"), `${JSON.stringify({ name: "hardgate", version, package: packageName, target: "x86_64-unknown-linux-gnu", commit })}\n`);
  const badArchive = path.join(fixture, "bad.tar.gz");
  const badPacked = spawnSync("tar", ["-czf", badArchive, "-C", badFixtureRoot, packageName], { encoding: "utf8" });
  if (badPacked.status !== 0) throw new Error(`bad metadata fixture failed: ${badPacked.stderr}`);
  fs.copyFileSync(badArchive, archive);
  writeChecksum(fs.readFileSync(archive));
  const metadataMismatch = runInstaller(path.join(fixture, "metadata-mismatch"));
  if (metadataMismatch.status === 0 || !metadataMismatch.stderr.includes("target/package")) {
    throw new Error(`metadata target mismatch unexpectedly passed:\n${metadataMismatch.stdout}${metadataMismatch.stderr}`);
  }
  fs.writeFileSync(path.join(badPackageRoot, "BUILD-METADATA.json"), `${JSON.stringify({ name: "hardgate", version, package: "hardgate-linux-x64", target: "x86_64-unknown-linux-musl", commit })}\n`);
  const badPackagePacked = spawnSync("tar", ["-czf", badArchive, "-C", badFixtureRoot, packageName], { encoding: "utf8" });
  if (badPackagePacked.status !== 0) throw new Error(`bad package fixture failed: ${badPackagePacked.stderr}`);
  fs.copyFileSync(badArchive, archive);
  writeChecksum(fs.readFileSync(archive));
  const packageMismatch = runInstaller(path.join(fixture, "package-mismatch"));
  if (packageMismatch.status === 0 || !packageMismatch.stderr.includes("target/package")) {
    throw new Error(`metadata package mismatch unexpectedly passed:\n${packageMismatch.stdout}${packageMismatch.stderr}`);
  }
  fs.writeFileSync(archive, goodArchive);
  fs.writeFileSync(path.join(fixture, "SHA256SUMS"), `deadbeef  ${packageName}.tar.gz\n`);
  const checksumMismatch = runInstaller(path.join(fixture, "checksum-mismatch"));
  if (checksumMismatch.status === 0 || !checksumMismatch.stderr.includes("checksum verification failed")) {
    throw new Error(`checksum mismatch unexpectedly passed:\n${checksumMismatch.stdout}${checksumMismatch.stderr}`);
  }
  fs.writeFileSync(path.join(fixture, "SHA256SUMS"), goodChecksums);
  console.log("release_contract.install.test: BusyBox checksum shim and installer identity OK");
} finally {
  fs.rmSync(fixture, { recursive: true, force: true });
}
