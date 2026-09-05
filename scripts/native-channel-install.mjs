// Isolated npm installation and consumer execution checks.
"use strict";

import fs from "node:fs";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { isRetryableNpmPackError, retryAfterMs } from "./npm-pack-retry.mjs";
import { childTimeoutMs, remainingMs, verificationPolicy } from "./npm-verification-policy.mjs";
import { validateProof } from "./native-channel-proof.mjs";
import { runReleaseProcess } from "./release-process.mjs";
import {
  PROOF_VERSION,
  PUBLIC_NPM_REGISTRY,
  WRAPPER_PACKAGE,
  digestFile,
  fail,
  hostNativePackage,
  needsNpmForce,
  nodeNpmPath,
  npmPackageSpec,
  pathInside,
  regularFile,
  restrictedPath,
  sanitizedEnvironment,
  stableExecutablePath,
} from "./native-channel-support.mjs";

const MAX_COMMAND_OUTPUT = 4 * 1024 * 1024;

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

function resetInstallPaths(paths) {
  for (const directory of [paths.prefix, paths.cache, paths.home, paths.temp]) fs.rmSync(directory, { recursive: true, force: true });
  for (const file of [paths.userConfig, paths.globalConfig]) fs.rmSync(file, { force: true });
}

function prepareInstall(paths) {
  for (const directory of [paths.prefix, paths.cache, paths.home, paths.temp]) fs.mkdirSync(directory, { recursive: true, mode: 0o700 });
  fs.writeFileSync(paths.userConfig, `registry=${PUBLIC_NPM_REGISTRY}\naudit=false\nfund=false\nignore-scripts=true\n`, { mode: 0o600 });
  fs.writeFileSync(paths.globalConfig, "", { mode: 0o600 });
}

function installEnvironment(paths) {
  return sanitizedEnvironment(undefined, {
    homeValue: paths.home,
    tempValue: paths.temp,
    npmConfig: {
      userConfig: paths.userConfig,
      globalConfig: paths.globalConfig,
      cache: paths.cache,
      prefix: paths.prefix,
      registry: PUBLIC_NPM_REGISTRY,
    },
  });
}

export async function installNpmPackage({
  packageName,
  version,
  mode,
  prefix,
  cache,
  userConfig,
  globalConfig,
  home = path.join(path.dirname(prefix), "home"),
  temp = path.join(path.dirname(prefix), "tmp"),
  force = false,
  npmCommand = nodeNpmPath(),
  runProcess = runReleaseProcess,
  policy = verificationPolicy(version),
  cwd,
}) {
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
  const paths = {prefix, cache, userConfig, globalConfig, home, temp};
  let lastError;
  for (let attempt = 1; attempt <= policy.attempts; attempt += 1) {
    if (attempt > 1) resetInstallPaths(paths);
    prepareInstall(paths);
    try {
      await runProcess(npmCommand, args, {
        cwd,
        env: installEnvironment(paths),
        timeoutMs: childTimeoutMs(policy),
        maxBuffer: MAX_COMMAND_OUTPUT,
      });
      return { prefix };
    } catch (error) {
      lastError = error;
      if (!isRetryableNpmPackError(error)) fail(`npm install for ${packageName} failed: ${error.message}`);
      if (attempt === policy.attempts) break;
      const pause = Math.max(policy.delayMs, retryAfterMs(error));
      if (pause >= remainingMs(policy)) fail(`npm install for ${packageName} retry exceeds the operation deadline`);
      await delay(pause);
    }
  }
  fail(`npm install for ${packageName} failed after ${policy.attempts} bounded attempts: ${lastError?.message ?? "retry limit"}`);
}

async function verifyVersion(binary, expected, { runProcess, policy, cwd, label, env }) {
  let output;
  try {
    output = await runProcess(binary, ["--version"], {
      cwd,
      env: env ?? sanitizedEnvironment(),
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
    home: path.join(base, "home"),
    temp: path.join(base, "tmp"),
  };
}

function createPathSentinel(prefix) {
  const directory = path.join(prefix, ".hardgate-private-path");
  const sentinel = path.join(directory, "hardgate");
  fs.mkdirSync(directory, { recursive: true, mode: 0o700 });
  const source = "/usr/bin/false";
  const stats = fs.statSync(source);
  if (!stats.isFile()) fail("private PATH sentinel source is not a regular file");
  fs.copyFileSync(source, sentinel);
  fs.chmodSync(sentinel, 0o755);
  return directory;
}

export async function verifyDirectConsumer({ values, host, policy, workRoot, install, runProcess, archiveEvidence }) {
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
  return { direct, expectedOutput };
}

export async function verifyWrapperConsumer({ values, host, policy, workRoot, install, runProcess, archiveEvidence, expectedOutput }) {
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
  if (!pathInside(wrapperPaths.prefix, wrapperRoot)) fail("wrapper package resolves outside the isolated install root");
  const wrapperManifest = readManifest(wrapperRoot, "wrapper");
  if (wrapperManifest.name !== WRAPPER_PACKAGE || wrapperManifest.version !== values.version) {
    fail(`wrapper package identity is ${wrapperManifest.name ?? "<missing>"}@${values.version}, expected ${WRAPPER_PACKAGE}@${values.version}`);
  }
  if (wrapperManifest.bin?.hardgate !== "bin/hardgate.js") fail("wrapper manifest bin.hardgate must be exactly bin/hardgate.js");
  const wrapperLauncher = checkedExecutable(wrapperPaths.prefix, path.join(wrapperRoot, "bin", "hardgate.js"), "wrapper launcher");
  if (!fs.readFileSync(wrapperLauncher).equals(fs.readFileSync(values.wrapperSource))) fail("installed wrapper launcher does not match --wrapper-source");
  const selectedName = hostNativePackage(host);
  if (selectedName !== values.packageName || wrapperManifest.optionalDependencies?.[selectedName] !== values.version) {
    fail(`wrapper optional dependency ${selectedName ?? "<missing>"} does not identify ${values.version}`);
  }
  const selected = verifyInstalledBinary({
    prefix: wrapperPaths.prefix,
    packageName: selectedName,
    version: values.version,
    expectedSha256: archiveEvidence.sha256,
    label: "wrapper native package",
  });
  const wrapperCommand = checkedExecutable(wrapperPaths.prefix, path.join(wrapperPaths.prefix, "node_modules", ".bin", "hardgate"), "wrapper command");
  const sentinelDirectory = createPathSentinel(wrapperPaths.prefix);
  const wrapperEnvironment = sanitizedEnvironment(undefined, {
    pathValue: `${sentinelDirectory}:${restrictedPath()}`,
    homeValue: wrapperPaths.home,
    tempValue: wrapperPaths.temp,
  });
  await verifyVersion(selected.real, expectedOutput, { runProcess, policy, cwd: wrapperPaths.prefix, label: "wrapper native package" });
  await verifyVersion(wrapperCommand, expectedOutput, { runProcess, policy, cwd: wrapperPaths.prefix, label: "wrapper command", env: wrapperEnvironment });
  return { executable: stableExecutablePath(selectedName), sha256: selected.sha256 };
}

export function channelProof({ values, direct, archiveSha256, wrapper }) {
  const proof = {
    schema_version: PROOF_VERSION,
    version: values.version,
    source_sha: values.sourceSha,
    mode: values.mode,
    package: values.packageName,
    archive: { name: `${values.packageName}.tar.gz`, sha256: archiveSha256 },
    consumer: {
      executable: stableExecutablePath(values.packageName),
      sha256: direct.sha256,
    },
  };
  if (wrapper) proof.wrapper = wrapper;
  return validateProof(proof);
}
