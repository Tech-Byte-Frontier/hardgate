// Verify the bounded archive and architecture evidence for one native package.
"use strict";

import { executableName } from "./release-platforms.mjs";
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { classifyBinaryAbi } from "./release-abi.mjs";
import { childTimeoutMs, verificationPolicy } from "./npm-verification-policy.mjs";
import { runReleaseProcess } from "./release-process.mjs";
import { archiveMemberMode, isExecutableMode } from "./release-support.mjs";
import {
  packageDescriptor,
  digestBytes,
  fail,
  regularFile,
  sanitizedEnvironment,
} from "./native-channel-support.mjs";

const MAX_BINARY_BYTES = 128 * 1024 * 1024;
const MAX_METADATA_BYTES = 64 * 1024;
const MAX_COMMAND_OUTPUT = 4 * 1024 * 1024;
const MAX_ARCHIVE_OUTPUT = MAX_BINARY_BYTES + 1024;
const SYSTEM_TAR = "/usr/bin/tar";
const SYSTEM_FILE = "/usr/bin/file";
const SYSTEM_READELF = "/usr/bin/readelf";

function runTarBytes(archive, member, { policy, maxBytes }) {
  const result = spawnSync(SYSTEM_TAR, ["-xOzf", archive, member], {
    encoding: null,
    timeout: childTimeoutMs(policy),
    killSignal: "SIGKILL",
    env: sanitizedEnvironment(),
    maxBuffer: Math.min(MAX_ARCHIVE_OUTPUT, maxBytes + 1),
  });
  if (result.error || result.status !== 0) {
    fail(`archive ${path.basename(archive)} lacks controlled member ${member}`);
  }
  if (!Buffer.isBuffer(result.stdout)) fail(`archive ${path.basename(archive)} returned non-binary member ${member}`);
  if (result.stdout.length > maxBytes) fail(`archive member ${member} exceeds its bounded size limit`);
  return result.stdout;
}

function archiveRecords(listing, packageName) {
  const expected = [`${packageName}/`, `${packageName}/BUILD-METADATA.json`, `${packageName}/${executableName(packageName)}`];
  const records = new Map();
  for (const member of expected) {
    const line = listing.split("\n").find((entry) => entry.trim().split(/\s+->\s+/, 1)[0].endsWith(` ${member}`));
    if (!line) fail(`archive is missing controlled member ${member}`);
    const mode = line.trim().split(/\s+/, 1)[0];
    if (typeof mode !== "string" || mode.length === 0) fail(`archive member ${member} has no file mode`);
    records.set(member, { mode });
  }
  if (records.get(`${packageName}/`).mode[0] !== "d") {
    fail(`${packageName} archive root must be a directory member`);
  }
  const metadata = records.get(`${packageName}/BUILD-METADATA.json`);
  if (metadata.mode[0] !== "-") fail(`${packageName} BUILD-METADATA.json must be a bounded regular file`);
  const binary = records.get(`${packageName}/${executableName(packageName)}`);
  if (binary.mode[0] !== "-") fail(`${packageName} hardgate must be a bounded regular file`);
  // GNU and BSD tar place size columns differently; runTarBytes enforces the
  // byte bounds on the actual extracted payload below.
  return records;
}

async function runText(command, args, { runProcess, policy, cwd } = {}) {
  try {
    return await runProcess(command, args, {
      cwd,
      timeoutMs: childTimeoutMs(policy),
      maxBuffer: MAX_COMMAND_OUTPUT,
      env: sanitizedEnvironment(),
    });
  } catch (error) {
    fail(`${command} ${args.join(" ")} failed: ${error.message}`);
  }
}

function verifyEmbeddedIdentity(bytes, { packageName, target, version, sourceSha }) {
  if (!bytes.includes(Buffer.from(version, "utf8")) || !bytes.includes(Buffer.from(sourceSha, "utf8"))) {
    fail(`${packageName} archive binary does not embed the expected version and source identity`);
  }
  if (!bytes.includes(Buffer.from(`${version} (${sourceSha})`, "utf8"))) {
    fail(`${packageName} archive binary does not embed the expected version/source identity marker`);
  }
  if (!bytes.includes(Buffer.from(`hardgate-target:${target}`, "utf8"))) {
    fail(`${packageName} archive binary does not embed the expected Cargo target marker ${target}`);
  }
}

async function verifyArchiveAbi(binaryPath, descriptor, { runProcess, policy }) {
  if (!descriptor.abi) return;
  const [report, programHeaders, symbols, notes] = await Promise.all([
    runText(SYSTEM_FILE, ["-b", binaryPath], { runProcess, policy }),
    descriptor.abi === "gnu" ? runText(SYSTEM_READELF, ["-l", binaryPath], { runProcess, policy }) : "",
    descriptor.abi === "gnu" ? runText(SYSTEM_READELF, ["-sW", binaryPath], { runProcess, policy }) : "",
    descriptor.abi === "gnu" ? runText(SYSTEM_READELF, ["-n", binaryPath], { runProcess, policy }) : "",
  ]);
  const evidence = classifyBinaryAbi({
    report,
    programHeaders,
    symbols,
    notes,
    abi: descriptor.abi,
  });
  if (!evidence.ok) fail(`${descriptor.name} ${descriptor.abi} ABI evidence failed: ${evidence.reason}`);
}

function assertArchiveMetadata(metadata, { packageName, descriptor, version, sourceSha }) {
  const expected = {
    name: "hardgate",
    version,
    target: descriptor.target,
    package: packageName,
    commit: sourceSha,
  };
  for (const [key, expectedValue] of Object.entries(expected)) {
    if (metadata[key] !== expectedValue) fail(`${packageName} metadata ${key} is ${metadata[key] ?? "<missing>"}`);
  }
}

export async function verifyNativeArchive({ archive, packageName, version, sourceSha, descriptor = packageDescriptor(packageName), directory, runProcess = runReleaseProcess, policy = verificationPolicy(version) }) {
  regularFile(archive, "--archive");
  if (path.basename(archive) !== `${packageName}.tar.gz`) fail(`archive must be named ${packageName}.tar.gz`);
  const listing = (await runText(SYSTEM_TAR, ["-tzf", archive], { runProcess, policy })).split("\n").filter(Boolean).sort();
  const expected = [`${packageName}/`, `${packageName}/BUILD-METADATA.json`, `${packageName}/${executableName(packageName)}`];
  if (listing.join("\n") !== expected.join("\n")) fail(`${packageName} archive contains unexpected members`);
  const modeListing = await runText(SYSTEM_TAR, ["-tvzf", archive], { runProcess, policy });
  archiveRecords(modeListing, packageName);
  if (!isExecutableMode(archiveMemberMode(modeListing, `${packageName}/${executableName(packageName)}`))) {
    fail(`${packageName} archive member hardgate must retain an executable mode`);
  }
  let metadata;
  try {
    metadata = JSON.parse(runTarBytes(archive, `${packageName}/BUILD-METADATA.json`, { policy, maxBytes: MAX_METADATA_BYTES }).toString("utf8"));
  } catch (error) {
    fail(`${packageName} BUILD-METADATA.json is not valid JSON: ${error.message}`);
  }
  assertArchiveMetadata(metadata, { packageName, descriptor, version, sourceSha });
  const bytes = runTarBytes(archive, `${packageName}/${executableName(packageName)}`, { policy, maxBytes: MAX_BINARY_BYTES });
  if (bytes.length === 0 || bytes.length > MAX_BINARY_BYTES) fail(`${packageName} archive binary is outside the bounded size limit`);
  verifyEmbeddedIdentity(bytes, { packageName, target: descriptor.target, version, sourceSha });
  const binaryPath = path.join(directory, `${packageName}.hardgate`);
  fs.writeFileSync(binaryPath, bytes, { mode: 0o755 });
  fs.chmodSync(binaryPath, 0o755);
  const report = await runText(SYSTEM_FILE, ["-b", binaryPath], { runProcess, policy });
  if (!descriptor.archPattern.test(report)) fail(`${packageName} architecture does not match ${descriptor.target}: ${report.trim()}`);
  await verifyArchiveAbi(binaryPath, descriptor, { runProcess, policy });
  return { sha256: digestBytes(bytes) };
}

export { regularFile };
