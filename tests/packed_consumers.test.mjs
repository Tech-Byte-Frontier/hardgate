// Exercise packed wrapper and optional-native archives through real npm and
// pnpm installs. No Cargo build or remote registry is involved.
"use strict";

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import zlib from "node:zlib";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

import { aggregateCleanupErrors, checkPackedConsumers } from "../scripts/check-packed-consumers.mjs";
import { inspectPackedArtifacts, snapshotArchiveFiles, verifyArchiveSnapshot, MAX_ARCHIVE_BYTES } from "../scripts/packed-consumer-artifacts.mjs";
import { startLocalRegistry } from "../scripts/packed-consumer-registry.mjs";
import { resolveInstalledPackage, resolveWrapperBinary, verifyResolvedNative } from "../scripts/packed-consumer-runtime.mjs";
import { runReleaseProcess } from "../scripts/release-process.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const fixtureRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-packed-consumer-test-"));

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: "utf8", timeout: 30_000, ...options });
  assert.equal(result.error, undefined, `${command} could not start: ${result.error?.message ?? "unknown error"}`);
  assert.equal(result.status, 0, `${command} ${args.join(" ")} failed\n${result.stdout}\n${result.stderr}`);
  return result;
}

async function runAsync(command, args, options = {}) {
  try {
    return await runReleaseProcess(command, args, {
      cwd: options.cwd,
      env: options.env,
      timeoutMs: 30_000,
      maxBuffer: 4 * 1024 * 1024,
    });
  } catch (error) {
    assert.fail(`${command} ${args.join(" ")} failed: ${error.message}\n${error.stdout ?? ""}\n${error.stderr ?? ""}`);
  }
}

function hash(file) {
  return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}

function fixtureEnvironment() {
  const npmCache = path.join(fixtureRoot, "npm-cache");
  const home = path.join(fixtureRoot, "home");
  fs.mkdirSync(npmCache, { recursive: true });
  fs.mkdirSync(home, { recursive: true });
  return {
    ...process.env,
    HOME: home,
    npm_config_cache: npmCache,
    NPM_CONFIG_CACHE: npmCache,
    npm_config_audit: "false",
    npm_config_fund: "false",
    npm_config_update_notifier: "false",
  };
}

function makeFixtureArchives() {
  const packagesDir = path.join(fixtureRoot, "packages");
  fs.mkdirSync(packagesDir, { recursive: true });
  const nativeSource = path.join(fixtureRoot, "fixture.c");
  const nativeBinary = path.join(fixtureRoot, "fixture-native");
  fs.writeFileSync(nativeSource, `#include <stdio.h>\n#include <string.h>\nint main(int argc, char **argv) {\n  if (argc == 2 && strcmp(argv[1], "--version") == 0) puts("hardgate 0.5.0 (aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa)");\n  return 0;\n}\n`);
  run("cc", ["-O2", nativeSource, "-o", nativeBinary]);
  fs.chmodSync(nativeBinary, 0o755);
  const names = [
    "hardgate-linux-x64",
    "hardgate-linux-x64-musl",
    "hardgate-linux-arm64",
    "hardgate-linux-arm64-musl",
    "hardgate-darwin-x64",
    "hardgate-darwin-arm64",
  ];
  const packageDirectories = ["hardgate", ...names];
  for (const name of packageDirectories) {
    const packageDirectory = path.join(fixtureRoot, name);
    fs.cpSync(path.join(root, "npm", name), packageDirectory, { recursive: true });
    if (name !== "hardgate") {
      fs.copyFileSync(nativeBinary, path.join(packageDirectory, "bin", "hardgate"));
      fs.chmodSync(path.join(packageDirectory, "bin", "hardgate"), 0o755);
    }
    run("npm", ["pack", "--json", "--loglevel=error", "--pack-destination", packagesDir], {
      cwd: packageDirectory,
      env: fixtureEnvironment(),
    });
  }
  return { packagesDir, nativeBinary };
}

function packModified(sourceName, label, mutate, mutateFiles = () => {}) {
  const packageDirectory = path.join(fixtureRoot, `bad-${label}`);
  const packagesDir = path.join(fixtureRoot, `bad-${label}-packages`);
  fs.cpSync(path.join(fixtureRoot, sourceName), packageDirectory, { recursive: true });
  const manifestPath = path.join(packageDirectory, "package.json");
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  mutate(manifest);
  fs.writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + "\n");
  mutateFiles(packageDirectory);
  fs.mkdirSync(packagesDir, { recursive: true });
  run("npm", ["pack", "--json", "--loglevel=error", "--pack-destination", packagesDir], {
    cwd: packageDirectory,
    env: fixtureEnvironment(),
  });
  const modifiedArchives = new Set(fs.readdirSync(packagesDir).filter((name) => name.endsWith(".tgz")));
  for (const archive of fs.readdirSync(path.join(fixtureRoot, "packages")).filter((name) => name.endsWith(".tgz"))) {
    if (!modifiedArchives.has(archive)) fs.copyFileSync(path.join(fixtureRoot, "packages", archive), path.join(packagesDir, archive));
  }
  return packagesDir;
}

function makeDuplicateManifestArchives() {
  const packagesDir = path.join(fixtureRoot, "duplicate-manifest-packages");
  fs.mkdirSync(packagesDir, { recursive: true });
  for (const archive of fs.readdirSync(path.join(fixtureRoot, "packages")).filter((name) => name.endsWith(".tgz"))) {
    fs.copyFileSync(path.join(fixtureRoot, "packages", archive), path.join(packagesDir, archive));
  }
  const wrapperArchive = fs.readdirSync(packagesDir).find((name) => name.endsWith(".tgz") && !/^hardgate-(?:linux|darwin)-/.test(name));
  assert.ok(wrapperArchive, "fixture wrapper archive must be present");
  const tarPath = path.join(fixtureRoot, "duplicate-manifest.tar");
  const duplicateRoot = path.join(fixtureRoot, "duplicate-manifest");
  const marker = path.join(fixtureRoot, "duplicate-hook-ran");
  fs.writeFileSync(tarPath, zlib.gunzipSync(fs.readFileSync(path.join(packagesDir, wrapperArchive))));
  fs.mkdirSync(path.join(duplicateRoot, "package"), { recursive: true });
  const manifest = JSON.parse(fs.readFileSync(path.join(fixtureRoot, "hardgate", "package.json"), "utf8"));
  manifest.scripts = { ...(manifest.scripts ?? {}), postinstall: `node -e "require('fs').writeFileSync(${JSON.stringify(marker)}, 'ran')"` };
  fs.writeFileSync(path.join(duplicateRoot, "package", "package.json"), JSON.stringify(manifest) + "\n");
  run("tar", ["-rf", tarPath, "-C", duplicateRoot, "package/package.json"]);
  fs.writeFileSync(path.join(packagesDir, wrapperArchive), zlib.gzipSync(fs.readFileSync(tarPath)));
  return { packagesDir, marker };
}

function makeUnsafeArchive() {
  const directory = path.join(fixtureRoot, "unsafe-member");
  fs.mkdirSync(directory, { recursive: true });
  const source = fs.readdirSync(path.join(fixtureRoot, "packages")).find((name) => name.endsWith(".tgz"));
  assert.ok(source, "fixture archive must be present");
  const tar = zlib.gunzipSync(fs.readFileSync(path.join(fixtureRoot, "packages", source)));
  const unsafe = Buffer.from("package//unsafe");
  tar.fill(0, 0, 100);
  unsafe.copy(tar, 0);
  fs.writeFileSync(path.join(directory, "unsafe.tgz"), zlib.gzipSync(tar));
  return directory;
}

function httpStatus(url) {
  return new Promise((resolve, reject) => {
    const request = http.get(url, (response) => {
      response.resume();
      response.once("end", () => resolve(response.statusCode));
    });
    request.once("error", reject);
  });
}

async function makeSkippedOptionalRoot(nativeBinary, registry) {
  const root = path.join(fixtureRoot, "skipped-optional");
  const ambient = path.join(fixtureRoot, "ambient-bin");
  const sentinel = path.join(fixtureRoot, "sentinel-bin");
  const cache = path.join(root, "cache");
  const home = path.join(root, "home");
  const userConfig = path.join(root, "npmrc");
  fs.mkdirSync(root, { recursive: true });
  fs.mkdirSync(ambient, { recursive: true });
  fs.mkdirSync(sentinel, { recursive: true });
  fs.writeFileSync(path.join(root, "package.json"), "{}\n");
  const env = {
    ...fixtureEnvironment(),
    HOME: home,
    NPM_CONFIG_CACHE: cache,
    npm_config_cache: cache,
    NPM_CONFIG_USERCONFIG: userConfig,
    npm_config_userconfig: userConfig,
    NPM_CONFIG_REGISTRY: registry.baseUrl,
    npm_config_registry: registry.baseUrl,
    NPM_CONFIG_OMIT: "optional",
    npm_config_omit: "optional",
    NPM_CONFIG_INCLUDE: "",
    npm_config_include: "",
    NPM_CONFIG_IGNORE_SCRIPTS: "false",
    npm_config_ignore_scripts: "false",
    HTTP_PROXY: "",
    HTTPS_PROXY: "",
    ALL_PROXY: "",
    http_proxy: "",
    https_proxy: "",
    all_proxy: "",
    NPM_CONFIG_PROXY: "",
    npm_config_proxy: "",
    NPM_CONFIG_HTTPS_PROXY: "",
    npm_config_https_proxy: "",
  };
  fs.mkdirSync(cache, { recursive: true });
  fs.mkdirSync(home, { recursive: true });
  fs.writeFileSync(userConfig, `registry=${registry.baseUrl}\ncache=${cache}\nomit=optional\nignore-scripts=false\naudit=false\nfund=false\n`);
  await runAsync("npm", ["install", "--no-audit", "--no-fund", "--omit=optional", "--registry", registry.baseUrl, "@tech-byte-frontier/hardgate@0.5.0"], {
    cwd: root,
    env,
  });
  fs.copyFileSync(nativeBinary, path.join(ambient, "hardgate"));
  fs.chmodSync(path.join(ambient, "hardgate"), 0o755);
  fs.writeFileSync(path.join(sentinel, "hardgate"), "#!/bin/sh\nexit 127\n", { mode: 0o755 });
  return {
    root,
    launcher: path.join(root, "node_modules", "@tech-byte-frontier", "hardgate", "bin", "hardgate.js"),
    environment: { PATH: [sentinel, ambient, path.dirname(process.execPath), "/usr/bin", "/bin"].join(path.delimiter) },
    ambient: path.join(ambient, "hardgate"),
  };
}

try {
  const fixture = makeFixtureArchives();
  const archiveFiles = fs.readdirSync(fixture.packagesDir).filter((name) => name.endsWith(".tgz"));
  const archiveHashes = new Map(archiveFiles.map((name) => [name, hash(path.join(fixture.packagesDir, name))]));
  const previousOverride = process.env.HARDGATE_BINARY;
  process.env.HARDGATE_BINARY = path.join(fixtureRoot, "does-not-exist");
  let report;
  try {
    report = await checkPackedConsumers({
      packagesDir: fixture.packagesDir,
      binary: fixture.nativeBinary,
      version: "0.5.0",
    });
  } finally {
    if (previousOverride === undefined) delete process.env.HARDGATE_BINARY;
    else process.env.HARDGATE_BINARY = previousOverride;
  }
  assert.equal(report.hostPackage, "hardgate-linux-x64");
  assert.equal(report.consumers.map((consumer) => consumer.manager).join(","), "npm,pnpm");
  assert.equal(report.consumers.every((consumer) => consumer.nativeSha256 === report.binarySha256), true);
  assert.match(report.expectedVersionOutput, /^hardgate 0\.5\.0 \([0-9a-f]+\)$/);
  for (const [name, expected] of archiveHashes) assert.equal(hash(path.join(fixture.packagesDir, name)), expected, `${name} was rewritten`);

  const missingHost = path.join(fixtureRoot, "missing-host");
  fs.mkdirSync(missingHost);
  for (const name of archiveFiles) {
    if (!name.startsWith("hardgate-linux-x64-0.5.0")) fs.copyFileSync(path.join(fixture.packagesDir, name), path.join(missingHost, name));
  }
  await assert.rejects(
    checkPackedConsumers({ packagesDir: missingHost, binary: fixture.nativeBinary, version: "0.5.0" }),
    /missing host optional dependency hardgate-linux-x64/,
  );

  const badUrl = packModified("hardgate-linux-x64", "url", (manifest) => {
    manifest.optionalDependencies = { "fixture-redirect": "https://registry.example.invalid/redirect.tgz" };
  });
  await assert.rejects(
    checkPackedConsumers({ packagesDir: badUrl, binary: fixture.nativeBinary, version: "0.5.0" }),
    /optionalDependency fixture-redirect must be a registry version/,
  );

  const badPlatformDeps = packModified("hardgate-linux-x64", "platform-deps", (manifest) => {
    manifest.optionalDependencies = { "fixture-extra": "0.5.0" };
  });
  await assert.rejects(
    checkPackedConsumers({ packagesDir: badPlatformDeps, binary: fixture.nativeBinary, version: "0.5.0" }),
    /hardgate-linux-x64 must not declare optionalDependencies/,
  );

  const badHook = packModified("hardgate", "hook", (manifest) => {
    manifest.scripts.postinstall = "curl https://registry.example.invalid/install";
  });
  await assert.rejects(
    checkPackedConsumers({ packagesDir: badHook, binary: fixture.nativeBinary, version: "0.5.0" }),
    /must not declare npm lifecycle hook postinstall/,
  );

  const badDescriptor = packModified("hardgate-darwin-x64", "descriptor", (manifest) => {
    manifest.cpu = ["arm64"];
  });
  await assert.rejects(
    checkPackedConsumers({ packagesDir: badDescriptor, binary: fixture.nativeBinary, version: "0.5.0" }),
    /hardgate-darwin-x64 manifest cpu=\["arm64"\] expected \["x64"\]/,
  );

  const missingNative = packModified("hardgate-darwin-x64", "missing-native", () => {}, (packageDirectory) => {
    fs.rmSync(path.join(packageDirectory, "bin", "hardgate"));
  });
  await assert.rejects(
    checkPackedConsumers({ packagesDir: missingNative, binary: fixture.nativeBinary, version: "0.5.0" }),
    /hardgate-darwin-x64@0\.5\.0\.tgz is missing package\/bin\/hardgate/,
  );

  const badBin = packModified("hardgate", "bin", (manifest) => {
    manifest.bin.hardgate = "bin/other.js";
  });
  await assert.rejects(
    checkPackedConsumers({ packagesDir: badBin, binary: fixture.nativeBinary, version: "0.5.0" }),
    /manifest bin\.hardgate must be exactly bin\/hardgate\.js/,
  );

  const missingNonHost = path.join(fixtureRoot, "missing-non-host");
  fs.mkdirSync(missingNonHost);
  for (const name of archiveFiles) {
    if (!name.startsWith("hardgate-darwin-x64-0.5.0")) fs.copyFileSync(path.join(fixture.packagesDir, name), path.join(missingNonHost, name));
  }
  await assert.rejects(
    checkPackedConsumers({ packagesDir: missingNonHost, binary: fixture.nativeBinary, version: "0.5.0" }),
    /missing: .*hardgate-darwin-x64@0\.5\.0/,
  );

  const duplicate = makeDuplicateManifestArchives();
  await assert.rejects(
    checkPackedConsumers({ packagesDir: duplicate.packagesDir, binary: fixture.nativeBinary, version: "0.5.0" }),
    /duplicate member: package\/package\.json/,
  );
  assert.equal(fs.existsSync(duplicate.marker), false, "duplicate manifest hook must not execute");

  assert.throws(
    () => inspectPackedArtifacts(makeUnsafeArchive(), "0.5.0", fixture.nativeBinary),
    /member path is not canonical/,
  );

  const inspected = inspectPackedArtifacts(fixture.packagesDir, "0.5.0", fixture.nativeBinary);
  const registry = await startLocalRegistry(inspected.artifacts);
  try {
    const skipped = await makeSkippedOptionalRoot(fixture.nativeBinary, registry);
    assert.throws(
      () => resolveInstalledPackage(skipped.root, "hardgate-linux-x64"),
      /installed optional dependency hardgate-linux-x64 is not resolvable/,
    );
    const ambientResolved = resolveWrapperBinary({ launcherPath: skipped.launcher, environment: skipped.environment });
    assert.equal(fs.realpathSync(ambientResolved), fs.realpathSync(skipped.ambient));
    assert.throws(
      () => verifyResolvedNative({ root: skipped.root, resolved: ambientResolved, label: "skipped optional" }),
      /escaped fresh consumer node_modules/,
    );
  } finally {
    await registry.close();
  }

  const limitedRegistry = await startLocalRegistry(inspected.artifacts, { maxRequests: 1, requestTimeoutMs: 1_000 });
  try {
    assert.equal(await httpStatus(`${limitedRegistry.baseUrl}missing-package`), 404);
    assert.equal(await httpStatus(`${limitedRegistry.baseUrl}missing-package`), 429);
  } finally {
    await limitedRegistry.close();
  }

  const ancestorRoot = path.join(fixtureRoot, "ancestor");
  fs.mkdirSync(path.join(ancestorRoot, "node_modules", "hardgate-linux-x64"), { recursive: true });
  fs.mkdirSync(path.join(ancestorRoot, "child", "node_modules"), { recursive: true });
  fs.writeFileSync(path.join(ancestorRoot, "child", "package.json"), "{}\n");
  fs.writeFileSync(path.join(ancestorRoot, "node_modules", "hardgate-linux-x64", "package.json"), JSON.stringify({ name: "hardgate-linux-x64", version: "0.5.0" }));
  await assert.rejects(
    Promise.resolve().then(() => resolveInstalledPackage(path.join(ancestorRoot, "child"), "hardgate-linux-x64")),
    /escaped fresh consumer node_modules/,
  );

  const oversizedDir = path.join(fixtureRoot, "oversized");
  fs.mkdirSync(oversizedDir);
  const oversized = path.join(oversizedDir, "oversized.tgz");
  const fd = fs.openSync(oversized, "w");
  fs.ftruncateSync(fd, MAX_ARCHIVE_BYTES + 1);
  fs.closeSync(fd);
  assert.throws(
    () => inspectPackedArtifacts(oversizedDir, "0.5.0", fixture.nativeBinary),
    /archive exceeds 67108864 bytes/,
  );

  const snapshot = snapshotArchiveFiles(inspected.artifacts);
  fs.appendFileSync(snapshot[0].path, "changed");
  assert.throws(() => verifyArchiveSnapshot(snapshot), /packed archive snapshot changed/);
  const primary = new Error("primary");
  assert.equal(aggregateCleanupErrors(primary, [new Error("cleanup")]).errors[0], primary);
  console.log("packed_consumers.test: OK");
} finally {
  fs.rmSync(fixtureRoot, { recursive: true, force: true });
}
