#!/usr/bin/env node
// Verify one published native npm package and, on Linux x64 GNU, its wrapper.
// Usage: node scripts/verify-native-channel.mjs --package NAME --version V
//   --source-sha SHA --archive FILE --mode exact|default --output PROOF.json
"use strict";

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { spawnSync } from "node:child_process";
import { classifyBinaryAbi } from "./release-abi.mjs";
import { childTimeoutMs, verificationPolicy } from "./npm-verification-policy.mjs";
import { runReleaseProcess } from "./release-process.mjs";
import { archiveMemberMode, isExecutableMode } from "./release-support.mjs";
import {
  NATIVE_PACKAGES,
  PROOF_VERSION,
  PUBLIC_NPM_REGISTRY,
  WRAPPER_PACKAGE,
  assertHostSupports,
  assertSourceSha,
  assertVersion,
  detectHost,
  digestBytes,
  digestFile,
  fail,
  hostNativePackage,
  needsNpmForce,
  npmPackageSpec,
  packageDescriptor,
  parseArgs,
  pathInside,
  sanitizedEnvironment,
  stableExecutablePath,
  validateProof,
  wrapperHost,
  writeProofAtomic,
} from "./native-channel-support.mjs";

const MAX_BINARY_BYTES = 128 * 1024 * 1024;
const MAX_COMMAND_OUTPUT = 4 * 1024 * 1024;
const MAX_ARCHIVE_OUTPUT = MAX_BINARY_BYTES + 1024;

function regularFile(file, label) {
  let stats;
  try {
    stats = fs.lstatSync(file);
  } catch (error) {
    fail(`${label} cannot be read: ${error.message}`);
  }
  if (stats.isSymbolicLink() || !stats.isFile()) fail(`${label} must be a regular file`);
  return stats;
}

function runTarBytes(archive, member) {
  const result = spawnSync("tar", ["-xOzf", archive, member], {
    encoding: null,
    timeout: 30_000,
    killSignal: "SIGKILL",
    maxBuffer: MAX_ARCHIVE_OUTPUT,
  });
  if (result.error || result.status !== 0) {
    fail(`archive ${path.basename(archive)} lacks controlled member ${member}`);
  }
  if (!Buffer.isBuffer(result.stdout)) fail(`archive ${path.basename(archive)} returned non-binary member ${member}`);
  return result.stdout;
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
  if (!bytes.includes(Buffer.from(`hardgate ${version} (${sourceSha})`, "utf8"))) {
    fail(`${packageName} archive binary does not embed the expected version/source identity marker`);
  }
  if (!bytes.includes(Buffer.from(`hardgate-target:${target}`, "utf8"))) {
    fail(`${packageName} archive binary does not embed the expected Cargo target marker ${target}`);
  }
}

async function verifyArchiveAbi(binaryPath, descriptor, { runProcess, policy }) {
  if (!descriptor.abi) return;
  const [report, programHeaders, symbols, notes] = await Promise.all([
    runText("file", ["-b", binaryPath], { runProcess, policy }),
    runText("readelf", ["-l", binaryPath], { runProcess, policy }),
    runText("readelf", ["-sW", binaryPath], { runProcess, policy }),
    runText("readelf", ["-n", binaryPath], { runProcess, policy }),
  ]);
  const evidence = classifyBinaryAbi({
    report,
    programHeaders,
    symbols,
    notes,
    abi: descriptor.abi,
    targetMarkerValid: descriptor.abi === "musl" && descriptor.target.endsWith("-musl"),
  });
  if (!evidence.ok) fail(`${descriptor.name} ${descriptor.abi} ABI evidence failed: ${evidence.reason}`);
}

export async function verifyNativeArchive({ archive, packageName, version, sourceSha, descriptor = packageDescriptor(packageName), directory, runProcess = runReleaseProcess, policy = verificationPolicy(version) }) {
  regularFile(archive, "--archive");
  const listing = (await runText("tar", ["-tzf", archive], { runProcess, policy })).split("\n").filter(Boolean).sort();
  const expected = [`${packageName}/`, `${packageName}/BUILD-METADATA.json`, `${packageName}/hardgate`];
  if (listing.join("\n") !== expected.join("\n")) {
    fail(`${packageName} archive contains unexpected members`);
  }
  const modeListing = await runText("tar", ["-tvzf", archive], { runProcess, policy });
  if (!isExecutableMode(archiveMemberMode(modeListing, `${packageName}/hardgate`))) {
    fail(`${packageName} archive member hardgate must retain an executable mode`);
  }
  let metadata;
  try {
    metadata = JSON.parse(runTarBytes(archive, `${packageName}/BUILD-METADATA.json`).toString("utf8"));
  } catch (error) {
    fail(`${packageName} BUILD-METADATA.json is not valid JSON: ${error.message}`);
  }
  for (const [key, expectedValue] of Object.entries({
    name: "hardgate",
    version,
    target: descriptor.target,
    package: packageName,
    commit: sourceSha,
  })) {
    if (metadata[key] !== expectedValue) fail(`${packageName} metadata ${key} is ${metadata[key] ?? "<missing>"}`);
  }
  const bytes = runTarBytes(archive, `${packageName}/hardgate`);
  if (bytes.length === 0 || bytes.length > MAX_BINARY_BYTES) fail(`${packageName} archive binary is outside the bounded size limit`);
  verifyEmbeddedIdentity(bytes, { packageName, target: descriptor.target, version, sourceSha });
  const binaryPath = path.join(directory, `${packageName}.hardgate`);
  fs.writeFileSync(binaryPath, bytes, { mode: 0o755 });
  fs.chmodSync(binaryPath, 0o755);
  const report = await runText("file", ["-b", binaryPath], { runProcess, policy });
  if (!descriptor.archPattern.test(report)) fail(`${packageName} architecture does not match ${descriptor.target}: ${report.trim()}`);
  await verifyArchiveAbi(binaryPath, descriptor, { runProcess, policy });
  return { sha256: digestBytes(bytes) };
}

function packageRoot(prefix, packageName) {
  return path.join(prefix, "node_modules", ...packageName.split("/"));
}

function readManifest(root, label) {
  const manifestPath = path.join(root, "package.json");
  regularFile(manifestPath, `${label} package.json`);
  try {
    return JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  } catch (error) {
    fail(`${label} package.json is not valid JSON: ${error.message}`);
  }
}

function checkedExecutable(prefix, file, label) {
  let stats;
  try {
    stats = fs.statSync(file);
  } catch (error) {
    fail(`${label} cannot be read: ${error.message}`);
  }
  if (!stats.isFile()) fail(`${label} must resolve to a regular file`);
  if ((stats.mode & 0o111) === 0) fail(`${label} is not executable`);
  if (!pathInside(prefix, file)) fail(`${label} resolves outside the isolated install root`);
  return fs.realpathSync(file);
}

function verifyInstalledBinary({ prefix, packageName, version, expectedSha256, label }) {
  const root = packageRoot(prefix, packageName);
  if (!pathInside(prefix, root)) fail(`${label} package resolves outside the isolated install root`);
  const manifest = readManifest(root, label);
  if (manifest.name !== packageName || manifest.version !== version) {
    fail(`${label} package identity is ${manifest.name ?? "<missing>"}@${manifest.version ?? "<missing>"}, expected ${packageName}@${version}`);
  }
  const logical = path.join(root, "bin", "hardgate");
  const real = checkedExecutable(prefix, logical, `${label} bin/hardgate`);
  const sha256 = digestFile(real);
  if (expectedSha256 !== undefined && sha256 !== expectedSha256) fail(`${label} binary bytes do not match the verified archive`);
  return { root, manifest, logical, real, sha256 };
}

export async function installNpmPackage({
  packageName,
  version,
  mode,
  prefix,
  cache,
  userConfig,
  globalConfig,
  force = false,
  npmCommand = "npm",
  runProcess = runReleaseProcess,
  policy,
  cwd,
}) {
  fs.mkdirSync(prefix, { recursive: true, mode: 0o700 });
  fs.mkdirSync(cache, { recursive: true, mode: 0o700 });
  fs.writeFileSync(userConfig, `registry=${PUBLIC_NPM_REGISTRY}\naudit=false\nfund=false\nignore-scripts=true\n`, { mode: 0o600 });
  fs.writeFileSync(globalConfig, "", { mode: 0o600 });
  const args = [
    "install",
    "--ignore-scripts",
    "--no-audit",
    "--no-fund",
    "--no-package-lock",
    "--prefix",
    prefix,
    "--cache",
    cache,
    "--userconfig",
    userConfig,
    "--globalconfig",
    globalConfig,
    "--registry",
    PUBLIC_NPM_REGISTRY,
  ];
  if (force) args.push("--force");
  args.push(npmPackageSpec(packageName, version, mode));
  try {
    await runProcess(npmCommand, args, {
      cwd,
      env: sanitizedEnvironment(),
      timeoutMs: childTimeoutMs(policy),
      maxBuffer: MAX_COMMAND_OUTPUT,
    });
  } catch (error) {
    fail(`npm install for ${packageName} failed: ${error.message}`);
  }
  return { prefix };
}

async function verifyVersion(binary, expected, { runProcess, policy, cwd, label }) {
  let output;
  try {
    output = await runProcess(binary, ["--version"], {
      cwd,
      env: sanitizedEnvironment(),
      timeoutMs: childTimeoutMs(policy),
      maxBuffer: 64 * 1024,
    });
  } catch (error) {
    fail(`${label} version command failed: ${error.message}`);
  }
  if (output.trim() !== expected) fail(`${label} reported ${JSON.stringify(output.trim())}, expected ${expected}`);
}

function installPaths(root, name) {
  const base = path.join(root, name);
  fs.mkdirSync(base, { recursive: true, mode: 0o700 });
  return {
    prefix: path.join(base, "prefix"),
    cache: path.join(base, "cache"),
    userConfig: path.join(base, "user.npmrc"),
    globalConfig: path.join(base, "global.npmrc"),
  };
}

function requestValues(request) {
  const packageName = request?.packageName ?? request?.package;
  const version = assertVersion(request?.version);
  const sourceSha = assertSourceSha(request?.sourceSha ?? request?.source_sha);
  const mode = request?.mode;
  if (mode !== "exact" && mode !== "default") fail(`mode must be exact or default, got ${mode || "<missing>"}`);
  if (typeof request?.archive !== "string" || request.archive.length === 0) fail("archive is required");
  const archive = path.resolve(request.archive);
  if (request.output !== undefined && (typeof request.output !== "string" || request.output.length === 0)) fail("output must be a non-empty path");
  const output = request.output === undefined ? undefined : path.resolve(request.output);
  if (output !== undefined && archive === output) fail("archive and output must identify different files");
  const descriptor = packageDescriptor(packageName);
  return { packageName: descriptor.name, descriptor, version, sourceSha, mode, archive, output };
}

export async function verifyNativeChannel(request, options = {}) {
  const values = requestValues(request);
  const host = options.host ?? detectHost();
  assertHostSupports(values.descriptor, host);
  regularFile(values.archive, "--archive");
  const policy = options.policy ?? verificationPolicy(values.version);
  const runProcess = options.runProcess ?? runReleaseProcess;
  const workRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-native-channel-"));
  const verifyArchive = options.verifyArchive ?? verifyNativeArchive;
  const install = options.installPackage ?? ((args) => installNpmPackage({ ...args, runProcess, npmCommand: options.npmCommand ?? "npm" }));
  const cleanup = options.cleanup ?? ((directory) => fs.rmSync(directory, { recursive: true, force: true }));
  let proof;
  let failure;
  try {
    const archiveEvidence = await verifyArchive({
      archive: values.archive,
      packageName: values.packageName,
      version: values.version,
      sourceSha: values.sourceSha,
      descriptor: values.descriptor,
      directory: workRoot,
      runProcess,
      policy,
    });
    if (!archiveEvidence || typeof archiveEvidence.sha256 !== "string" || !/^[0-9a-f]{64}$/.test(archiveEvidence.sha256)) {
      fail("archive verifier did not return a lowercase SHA256 digest");
    }
    const directPaths = installPaths(workRoot, "direct");
    await install({
      packageName: values.packageName,
      version: values.version,
      mode: values.mode,
      force: needsNpmForce(values.descriptor, host),
      ...directPaths,
      policy,
      cwd: workRoot,
    });
    const direct = verifyInstalledBinary({
      prefix: directPaths.prefix,
      packageName: values.packageName,
      version: values.version,
      expectedSha256: archiveEvidence.sha256,
      label: "native package",
    });
    const expectedOutput = `hardgate ${values.version} (${values.sourceSha})`;
    await verifyVersion(direct.real, expectedOutput, { runProcess, policy, cwd: directPaths.prefix, label: "native package" });
    proof = {
      version: values.version,
      source_sha: values.sourceSha,
      mode: values.mode,
      package: values.packageName,
      consumer: {
        executable: stableExecutablePath(values.packageName),
        sha256: direct.sha256,
      },
    };
    if (wrapperHost(host)) {
      const wrapperPaths = installPaths(workRoot, "wrapper");
      await install({
        packageName: WRAPPER_PACKAGE,
        version: values.version,
        mode: values.mode,
        force: false,
        ...wrapperPaths,
        policy,
        cwd: workRoot,
      });
      const wrapperRoot = packageRoot(wrapperPaths.prefix, WRAPPER_PACKAGE);
      const wrapperManifest = readManifest(wrapperRoot, "wrapper");
      if (wrapperManifest.name !== WRAPPER_PACKAGE || wrapperManifest.version !== values.version) {
        fail(`wrapper package identity is ${wrapperManifest.name ?? "<missing>"}@${wrapperManifest.version ?? "<missing>"}, expected ${WRAPPER_PACKAGE}@${values.version}`);
      }
      const selectedName = hostNativePackage(host);
      if (!selectedName || wrapperManifest.optionalDependencies?.[selectedName] !== values.version) {
        fail(`wrapper optional dependency ${selectedName ?? "<missing>"} does not identify ${values.version}`);
      }
      const selected = verifyInstalledBinary({
        prefix: wrapperPaths.prefix,
        packageName: selectedName,
        version: values.version,
        expectedSha256: selectedName === values.packageName ? archiveEvidence.sha256 : undefined,
        label: "wrapper native package",
      });
      const wrapperCommand = checkedExecutable(wrapperPaths.prefix, path.join(wrapperPaths.prefix, "node_modules", ".bin", "hardgate"), "wrapper command");
      await verifyVersion(selected.real, expectedOutput, { runProcess, policy, cwd: wrapperPaths.prefix, label: "wrapper native package" });
      await verifyVersion(wrapperCommand, expectedOutput, { runProcess, policy, cwd: wrapperPaths.prefix, label: "wrapper command" });
      proof.wrapper = {
        executable: stableExecutablePath(selectedName),
        sha256: selected.sha256,
      };
    }
    proof = validateProof(proof);
  } catch (error) {
    failure = error;
  }
  try {
    await cleanup(workRoot);
  } catch (error) {
    failure ??= error;
  }
  if (failure) throw failure;
  if (values.output !== undefined) writeProofAtomic(values.output, proof);
  return proof;
}

async function main(argv = process.argv.slice(2)) {
  const request = parseArgs(argv);
  await verifyNativeChannel(request);
  console.log(`verified ${request.packageName} ${request.mode} consumer channel`);
}

const invokedPath = process.argv[1];
if (invokedPath && import.meta.url === pathToFileURL(invokedPath).href) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
}

export { NATIVE_PACKAGES, PROOF_VERSION, PUBLIC_NPM_REGISTRY, WRAPPER_PACKAGE };
