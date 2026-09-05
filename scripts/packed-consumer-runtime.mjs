// Isolated package-manager environments and installed-wrapper verification.
"use strict";

import fs from "node:fs";
import crypto from "node:crypto";
import path from "node:path";
import { createRequire } from "node:module";

import { runReleaseProcess } from "./release-process.mjs";
import { WRAPPER_NAME } from "./packed-consumer-artifacts.mjs";

const PROCESS_TIMEOUT_MS = 120_000;
const PROCESS_OUTPUT_BYTES = 4 * 1024 * 1024;

function fail(message) {
  throw new Error(message);
}

function managerPath(name) {
  const entries = (process.env.PATH ?? "").split(path.delimiter).filter(Boolean);
  for (const entry of entries) {
    const candidate = path.join(entry, name);
    try {
      const stat = fs.statSync(candidate);
      if (stat.isFile() && (stat.mode & 0o111) !== 0) return candidate;
    } catch {
      // Try the next PATH entry.
    }
  }
  fail(`could not find ${name} on PATH`);
}

function clearAmbientConfig(env) {
  for (const key of Object.keys(env)) {
    if (/^(?:NPM_CONFIG_|npm_config_|PNPM_CONFIG_|pnpm_config_)/.test(key)) delete env[key];
  }
  for (const key of [
    "HARDGATE_BINARY", "HARDGATE_BINARY_PATH", "HARDGATE_LAUNCHER_DEPTH", "NODE_PATH", "NODE_OPTIONS",
    "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy",
  ]) delete env[key];
}

function privateRuntimePath(root) {
  const directory = path.join(root, "private-bin");
  const sentinel = path.join(directory, "hardgate");
  fs.mkdirSync(directory, { recursive: true });
  fs.writeFileSync(sentinel, "#!/bin/sh\nexit 127\n", { mode: 0o755 });
  return [directory, path.dirname(process.execPath), "/usr/local/bin", "/usr/bin", "/bin"].join(path.delimiter);
}

function writeNpmConfig(file, registryUrl, cache) {
  fs.writeFileSync(file, [
    `registry=${registryUrl}`, `cache=${cache}`, "audit=false", "fund=false",
    "update-notifier=false", "fetch-retries=0", "fetch-timeout=10000", "include=optional",
    "optional=true", "ignore-scripts=false", "proxy=", "https-proxy=",
    "noproxy=127.0.0.1,localhost", "",
  ].join("\n"));
}

function cleanConsumerEnvironment(root, registryUrl, cache, store) {
  const env = { ...process.env };
  clearAmbientConfig(env);
  const nodeDirectory = path.dirname(process.execPath);
  env.PATH = [nodeDirectory, "/usr/local/bin", "/usr/bin", "/bin"].join(path.delimiter);
  env.HOME = path.join(root, "home");
  env.XDG_CONFIG_HOME = path.join(root, "config");
  env.PNPM_HOME = path.join(root, "pnpm-home");
  env.NPM_CONFIG_USERCONFIG = path.join(root, "npmrc");
  env.NPM_CONFIG_CACHE = cache;
  env.NPM_CONFIG_REGISTRY = registryUrl;
  env.NPM_CONFIG_AUDIT = "false";
  env.NPM_CONFIG_FUND = "false";
  env.NPM_CONFIG_UPDATE_NOTIFIER = "false";
  env.NPM_CONFIG_FETCH_RETRIES = "0";
  env.NPM_CONFIG_FETCH_TIMEOUT = "10000";
  env.NPM_CONFIG_INCLUDE = "optional";
  env.NPM_CONFIG_OMIT = "";
  env.NPM_CONFIG_OPTIONAL = "true";
  env.NPM_CONFIG_IGNORE_SCRIPTS = "false";
  env.NPM_CONFIG_PROXY = "";
  env.NPM_CONFIG_HTTPS_PROXY = "";
  env.NPM_CONFIG_NO_PROXY = "127.0.0.1,localhost";
  env.PNPM_STORE_DIR = store;
  env.PNPM_CONFIG_REGISTRY = registryUrl;
  env.PNPM_CONFIG_IGNORE_SCRIPTS = "false";
  env.PNPM_CONFIG_OPTIONAL = "true";
  env.npm_config_userconfig = env.NPM_CONFIG_USERCONFIG;
  env.npm_config_cache = cache;
  env.npm_config_registry = registryUrl;
  env.npm_config_audit = "false";
  env.npm_config_fund = "false";
  env.npm_config_update_notifier = "false";
  env.npm_config_fetch_retries = "0";
  env.npm_config_fetch_timeout = "10000";
  env.npm_config_include = "optional";
  env.npm_config_omit = "";
  env.npm_config_optional = "true";
  env.npm_config_ignore_scripts = "false";
  env.npm_config_proxy = "";
  env.npm_config_https_proxy = "";
  env.npm_config_no_proxy = "127.0.0.1,localhost";
  env.pnpm_store_dir = store;
  env.pnpm_config_registry = registryUrl;
  env.pnpm_config_ignore_scripts = "false";
  env.pnpm_config_optional = "true";
  env.CI = "1";
  fs.mkdirSync(env.HOME, { recursive: true });
  fs.mkdirSync(env.XDG_CONFIG_HOME, { recursive: true });
  fs.mkdirSync(env.PNPM_HOME, { recursive: true });
  fs.mkdirSync(cache, { recursive: true });
  fs.mkdirSync(store, { recursive: true });
  writeNpmConfig(env.NPM_CONFIG_USERCONFIG, registryUrl, cache);
  return env;
}

function invocationEnvironment(root, installEnvironment) {
  return { ...installEnvironment, PATH: privateRuntimePath(root) };
}

export function createConsumerRoot(parent, manager) {
  const root = path.join(parent, manager);
  fs.mkdirSync(root, { recursive: true });
  fs.writeFileSync(path.join(root, "package.json"), JSON.stringify({
    name: `hardgate-packed-${manager}-consumer`, version: "1.0.0", private: true,
  }, null, 2) + "\n");
  return root;
}

function installedNodeModules(root) {
  const directory = path.join(root, "node_modules");
  try {
    return fs.realpathSync(directory);
  } catch (error) {
    fail(`fresh consumer node_modules is missing: ${directory} (${error.message})`);
  }
}

function installedPath(root, candidate, label) {
  const modules = installedNodeModules(root);
  let resolved;
  try {
    resolved = fs.realpathSync(candidate);
  } catch (error) {
    fail(`${label} is not inside fresh consumer node_modules: ${candidate} (${error.message})`);
  }
  const relative = path.relative(modules, resolved);
  if (relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) fail(`${label} escaped fresh consumer node_modules: ${resolved}`);
  return resolved;
}

export function resolveInstalledPackage(root, packageName, from = path.join(root, "package.json")) {
  const requireFromRoot = createRequire(from);
  let manifestPath;
  try {
    manifestPath = requireFromRoot.resolve(`${packageName}/package.json`);
  } catch (error) {
    fail(`installed optional dependency ${packageName} is not resolvable: ${error.message}`);
  }
  const resolvedPath = installedPath(root, manifestPath, `installed ${packageName} manifest`);
  return { path: resolvedPath, manifest: JSON.parse(fs.readFileSync(resolvedPath, "utf8")) };
}

function installedPackageBinary(root, packageName, from) {
  const packageManifest = resolveInstalledPackage(root, packageName, from);
  const binary = path.join(path.dirname(packageManifest.path), "bin", "hardgate");
  let stat;
  try {
    stat = fs.statSync(installedPath(root, binary, `installed ${packageName} binary`));
  } catch (error) {
    fail(`installed ${packageName} binary is missing: ${binary} (${error.message})`);
  }
  if (!stat.isFile() || (stat.mode & 0o111) === 0) fail(`installed ${packageName} binary is not executable: ${binary}`);
  return { ...packageManifest, binary: installedPath(root, binary, `installed ${packageName} binary`) };
}

function withEnvironment(environment, operation) {
  const previous = { ...process.env };
  try {
    for (const key of Object.keys(process.env)) delete process.env[key];
    Object.assign(process.env, environment);
    return operation();
  } finally {
    for (const key of Object.keys(process.env)) delete process.env[key];
    Object.assign(process.env, previous);
  }
}

export function resolveWrapperBinary({ launcherPath, environment }) {
  const requireLauncher = createRequire(launcherPath);
  const launcher = requireLauncher(launcherPath);
  if (typeof launcher.findBinary !== "function") fail(`installed wrapper does not export findBinary: ${launcherPath}`);
  return withEnvironment(environment, () => launcher.findBinary());
}

export function verifyResolvedNative({ root, resolved, expectedNative, label }) {
  if (!resolved) fail(`${label} did not resolve an installed native binary`);
  const actual = installedPath(root, resolved, `${label} native resolution`);
  if (expectedNative) {
    const expected = installedPath(root, expectedNative, `${label} expected native`);
    if (actual !== expected) fail(`${label} resolved ${actual}; expected installed native ${expected}`);
  }
  return actual;
}

async function boundedProcess(command, args, options, label) {
  try {
    return await runReleaseProcess(command, args, {
      cwd: options.cwd, env: options.env, timeoutMs: PROCESS_TIMEOUT_MS, maxBuffer: PROCESS_OUTPUT_BYTES,
    });
  } catch (error) {
    const stdout = error.stdout ? `\nstdout:\n${error.stdout}` : "";
    const stderr = error.stderr ? `\nstderr:\n${error.stderr}` : "";
    fail(`${label} failed: ${error.message}${stdout}${stderr}`);
  }
}

function verifyInstalledPackage(packageValue, expectedName, expectedVersion, label) {
  if (packageValue.manifest.name !== expectedName || packageValue.manifest.version !== expectedVersion) {
    fail(`${label} identity is ${packageValue.manifest.name}@${packageValue.manifest.version}; expected ${expectedName}@${expectedVersion}`);
  }
}

export async function installAndVerify({ manager, root, registry, version, host, wrapperLauncherBytes, expectedOutput, expectedHash, tempRoot }) {
  const cache = path.join(tempRoot, `${manager}-cache`);
  const store = path.join(tempRoot, `${manager}-store`);
  const env = cleanConsumerEnvironment(root, registry.baseUrl, cache, store);
  const managerExecutable = managerPath(manager);
  const spec = `${WRAPPER_NAME}@${version}`;
  const args = manager === "npm"
    ? ["install", "--no-audit", "--no-fund", "--include=optional", "--registry", registry.baseUrl, spec]
    : ["add", "--registry", registry.baseUrl, "--store-dir", store, spec];
  await boundedProcess(managerExecutable, args, { cwd: root, env }, `${manager} packed consumer install`);
  const wrapper = resolveInstalledPackage(root, WRAPPER_NAME);
  verifyInstalledPackage(wrapper, WRAPPER_NAME, version, `${manager} installed wrapper`);
  const installedLauncher = installedPath(root, path.join(path.dirname(wrapper.path), "bin", "hardgate.js"), `${manager} installed wrapper launcher`);
  if (!fs.readFileSync(installedLauncher).equals(wrapperLauncherBytes)) fail(`${manager} installed wrapper launcher bytes differ from the packed wrapper archive`);
  const nativePackage = installedPackageBinary(root, host, wrapper.path);
  verifyInstalledPackage(nativePackage, host, version, `${manager} installed native`);
  const installedHash = crypto.createHash("sha256").update(fs.readFileSync(nativePackage.binary)).digest("hex");
  if (installedHash !== expectedHash) fail(`${manager} resolved ${host} digest ${installedHash} does not match expected ${expectedHash}`);
  const wrapperBinary = path.join(root, "node_modules", ".bin", "hardgate");
  const installedWrapperBinary = installedPath(root, wrapperBinary, `${manager} installed wrapper .bin entry`);
  let wrapperStat;
  try {
    wrapperStat = fs.statSync(installedWrapperBinary);
  } catch (error) {
    fail(`${manager} installed wrapper .bin entry is missing: ${error.message}`);
  }
  if (!wrapperStat.isFile() || (wrapperStat.mode & 0o111) === 0) fail(`${manager} installed wrapper .bin entry is not executable`);
  const invocationEnv = invocationEnvironment(root, env);
  const resolvedNative = resolveWrapperBinary({ launcherPath: installedLauncher, environment: invocationEnv });
  verifyResolvedNative({ root, resolved: resolvedNative, expectedNative: nativePackage.binary, label: `${manager} wrapper` });
  const output = (await boundedProcess(installedWrapperBinary, ["--version"], { cwd: root, env: invocationEnv }, `${manager} packed consumer invocation`)).trim();
  if (output !== expectedOutput) fail(`${manager} installed wrapper reported ${JSON.stringify(output)}; expected ${JSON.stringify(expectedOutput)}`);
  return { manager, nativeSha256: installedHash, versionOutput: output };
}

export async function expectedVersion(binary, version, tempRoot) {
  const env = cleanConsumerEnvironment(tempRoot, "http://127.0.0.1/", path.join(tempRoot, "expected-cache"), path.join(tempRoot, "expected-store"));
  const output = (await boundedProcess(binary, ["--version"], { cwd: tempRoot, env }, "expected native binary invocation")).trim();
  if (!output.startsWith(`hardgate ${version} (`) || !output.endsWith(")")) fail(`--binary --version output does not identify ${version}: ${JSON.stringify(output)}`);
  return output;
}
