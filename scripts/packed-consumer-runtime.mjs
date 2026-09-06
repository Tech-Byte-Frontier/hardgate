// Isolated package-manager environments and installed-wrapper verification.
"use strict";

import fs from "node:fs";
import crypto from "node:crypto";
import path from "node:path";
import { createRequire } from "node:module";

import { verifyInstalledCheck } from "./installed-check.mjs";
import { runReleaseProcess } from "./release-process.mjs";
import { WRAPPER_NAME } from "./packed-consumer-artifacts.mjs";

const PROCESS_TIMEOUT_MS = 120_000;
const PROCESS_OUTPUT_BYTES = 4 * 1024 * 1024;

function fail(message) {
  throw new Error(message);
}

export function managerPath(name) {
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
    "NODE_AUTH_TOKEN", "NPM_TOKEN", "CARGO_REGISTRY_TOKEN", "ACTIONS_ID_TOKEN_REQUEST_TOKEN", "ACTIONS_ID_TOKEN_REQUEST_URL",
    "LD_PRELOAD", "LD_LIBRARY_PATH", "NODE_EXTRA_CA_CERTS", "SSL_CERT_FILE", "SSL_CERT_DIR",
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

function environmentPaths(root, cache, store) {
  const nodeDirectory = path.dirname(process.execPath);
  return {
    PATH: [nodeDirectory, "/usr/local/bin", "/usr/bin", "/bin"].join(path.delimiter),
    HOME: path.join(root, "home"),
    XDG_CONFIG_HOME: path.join(root, "config"),
    PNPM_HOME: path.join(root, "pnpm-home"),
    NPM_CONFIG_USERCONFIG: path.join(root, "npmrc"),
    NPM_CONFIG_CACHE: cache,
    PNPM_STORE_DIR: store,
  };
}

function configureNpmEnvironment(env, registryUrl, cache) {
  const values = {
    NPM_CONFIG_REGISTRY: registryUrl, NPM_CONFIG_AUDIT: "false", NPM_CONFIG_FUND: "false",
    NPM_CONFIG_UPDATE_NOTIFIER: "false", NPM_CONFIG_FETCH_RETRIES: "0", NPM_CONFIG_FETCH_TIMEOUT: "10000",
    NPM_CONFIG_INCLUDE: "optional", NPM_CONFIG_OMIT: "", NPM_CONFIG_OPTIONAL: "true",
    NPM_CONFIG_IGNORE_SCRIPTS: "false", NPM_CONFIG_PROXY: "", NPM_CONFIG_HTTPS_PROXY: "",
    NPM_CONFIG_NO_PROXY: "127.0.0.1,localhost", npm_config_registry: registryUrl,
    npm_config_audit: "false", npm_config_fund: "false", npm_config_update_notifier: "false",
    npm_config_fetch_retries: "0", npm_config_fetch_timeout: "10000", npm_config_include: "optional",
    npm_config_omit: "", npm_config_optional: "true", npm_config_ignore_scripts: "false",
    npm_config_proxy: "", npm_config_https_proxy: "", npm_config_no_proxy: "127.0.0.1,localhost",
    npm_config_cache: cache,
  };
  Object.assign(env, values, { npm_config_userconfig: env.NPM_CONFIG_USERCONFIG });
}

function configurePnpmEnvironment(env, registryUrl, store) {
  Object.assign(env, {
    PNPM_CONFIG_REGISTRY: registryUrl, PNPM_CONFIG_IGNORE_SCRIPTS: "false", PNPM_CONFIG_OPTIONAL: "true",
    pnpm_config_registry: registryUrl, pnpm_config_ignore_scripts: "false", pnpm_config_optional: "true",
    pnpm_store_dir: store,
  });
}

function prepareConsumerDirectories(env, cache, store) {
  for (const directory of [env.HOME, env.XDG_CONFIG_HOME, env.PNPM_HOME, cache, store]) {
    fs.mkdirSync(directory, { recursive: true });
  }
}

export function cleanConsumerEnvironment(root, registryUrl, cache, store) {
  const env = { ...process.env };
  clearAmbientConfig(env);
  Object.assign(env, environmentPaths(root, cache, store));
  configureNpmEnvironment(env, registryUrl, cache);
  configurePnpmEnvironment(env, registryUrl, store);
  env.CI = "1";
  prepareConsumerDirectories(env, cache, store);
  writeNpmConfig(env.NPM_CONFIG_USERCONFIG, registryUrl, cache);
  return env;
}

export function invocationEnvironment(root, installEnvironment) {
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

function wrapperBinTarget(root, entry, launcher) {
  const resolved = fs.realpathSync(entry);
  if (resolved === launcher) return resolved;
  let shim;
  try {
    shim = fs.readFileSync(entry, "utf8");
  } catch (error) {
    fail(`installed wrapper .bin entry cannot be read: ${entry} (${error.message})`);
  }
  const target = shim.match(/^# cmd-shim-target=(.+)$/m)?.[1]?.trim();
  if (!target) fail(`installed wrapper .bin entry does not resolve to checked wrapper launcher: ${entry}`);
  const resolvedTarget = installedPath(root, target, "installed wrapper .bin target");
  if (resolvedTarget !== launcher) {
    fail(`installed wrapper .bin target resolves to ${resolvedTarget}; expected ${launcher}`);
  }
  return resolvedTarget;
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

export async function boundedProcess(command, args, options, label) {
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

function installArguments(manager, registry, store, spec) {
  if (manager === "npm") return ["install", "--no-audit", "--no-fund", "--include=optional", "--registry", registry, spec];
  return ["add", "--registry", registry, "--store-dir", store, spec];
}

function verifyWrapperInstall({ manager, root, version, wrapperLauncherBytes }) {
  const wrapper = resolveInstalledPackage(root, WRAPPER_NAME);
  verifyInstalledPackage(wrapper, WRAPPER_NAME, version, `${manager} installed wrapper`);
  const launcher = installedPath(root, path.join(path.dirname(wrapper.path), "bin", "hardgate.js"), `${manager} installed wrapper launcher`);
  if (!fs.readFileSync(launcher).equals(wrapperLauncherBytes)) fail(`${manager} installed wrapper launcher bytes differ from the packed wrapper archive`);
  return { wrapper, launcher };
}

function verifyNativeInstall({ manager, root, host, expectedHash, wrapper }) {
  const nativePackage = installedPackageBinary(root, host, wrapper.path);
  verifyInstalledPackage(nativePackage, host, wrapper.manifest.version, `${manager} installed native`);
  const installedHash = crypto.createHash("sha256").update(fs.readFileSync(nativePackage.binary)).digest("hex");
  if (installedHash !== expectedHash) fail(`${manager} resolved ${host} digest ${installedHash} does not match expected ${expectedHash}`);
  return { nativePackage, installedHash };
}

function verifyWrapperEntry({ manager, root, launcher }) {
  const wrapperBinary = path.join(root, "node_modules", ".bin", "hardgate");
  const installedWrapperBinary = installedPath(root, wrapperBinary, `${manager} installed wrapper .bin entry`);
  wrapperBinTarget(root, installedWrapperBinary, launcher);
  let wrapperStat;
  try {
    wrapperStat = fs.statSync(installedWrapperBinary);
  } catch (error) {
    fail(`${manager} installed wrapper .bin entry is missing: ${error.message}`);
  }
  if (!wrapperStat.isFile() || (wrapperStat.mode & 0o111) === 0) fail(`${manager} installed wrapper .bin entry is not executable`);
  return installedWrapperBinary;
}

export async function installAndVerify({ manager, root, registry, version, host, wrapperLauncherBytes, expectedOutput, expectedHash, tempRoot }) {
  const cache = path.join(tempRoot, `${manager}-cache`);
  const store = path.join(tempRoot, `${manager}-store`);
  const env = cleanConsumerEnvironment(root, registry.baseUrl, cache, store);
  const spec = `${WRAPPER_NAME}@${version}`;
  await boundedProcess(managerPath(manager), installArguments(manager, registry.baseUrl, store, spec), { cwd: root, env }, `${manager} packed consumer install`);
  const { wrapper, launcher } = verifyWrapperInstall({ manager, root, version, wrapperLauncherBytes });
  const { nativePackage, installedHash } = verifyNativeInstall({ manager, root, host, expectedHash, wrapper });
  const installedWrapperBinary = verifyWrapperEntry({ manager, root, launcher });
  const invocationEnv = invocationEnvironment(root, env);
  const resolvedNative = resolveWrapperBinary({ launcherPath: launcher, environment: invocationEnv });
  verifyResolvedNative({ root, resolved: resolvedNative, expectedNative: nativePackage.binary, label: `${manager} wrapper` });
  const output = (await boundedProcess(installedWrapperBinary, ["--version"], { cwd: root, env: invocationEnv }, `${manager} packed consumer invocation`)).trim();
  if (output !== expectedOutput) fail(`${manager} installed wrapper reported ${JSON.stringify(output)}; expected ${JSON.stringify(expectedOutput)}`);
  const acceptance = await verifyInstalledCheck(installedWrapperBinary, { parent: tempRoot, env: invocationEnv });
  return { manager, scope: "project", nativeSha256: installedHash, versionOutput: output, acceptance };
}

export async function expectedVersion(binary, version, tempRoot) {
  const env = cleanConsumerEnvironment(tempRoot, "http://127.0.0.1/", path.join(tempRoot, "expected-cache"), path.join(tempRoot, "expected-store"));
  const output = (await boundedProcess(binary, ["--version"], { cwd: tempRoot, env }, "expected native binary invocation")).trim();
  if (!output.startsWith(`hardgate ${version} (`) || !output.endsWith(")")) fail(`--binary --version output does not identify ${version}: ${JSON.stringify(output)}`);
  return output;
}
