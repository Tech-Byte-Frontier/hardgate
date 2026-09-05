// Local archives and consumer roots used by the packed-consumer acceptance test.
"use strict";

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import zlib from "node:zlib";
import { spawnSync } from "node:child_process";

import { runReleaseProcess } from "../scripts/release-process.mjs";

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: "utf8", timeout: 30_000, ...options });
  assert.equal(result.error, undefined, `${command} could not start: ${result.error?.message ?? "unknown error"}`);
  assert.equal(result.status, 0, `${command} ${args.join(" ")} failed\n${result.stdout}\n${result.stderr}`);
  return result;
}

async function runAsync(command, args, options = {}) {
  try {
    return await runReleaseProcess(command, args, {
      cwd: options.cwd, env: options.env, timeoutMs: 30_000, maxBuffer: 4 * 1024 * 1024,
    });
  } catch (error) {
    assert.fail(`${command} ${args.join(" ")} failed: ${error.message}\n${error.stdout ?? ""}\n${error.stderr ?? ""}`);
  }
}

export function hash(file) {
  return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}

function fixtureEnvironment(fixtureRoot) {
  const npmCache = path.join(fixtureRoot, "npm-cache");
  const home = path.join(fixtureRoot, "home");
  fs.mkdirSync(npmCache, { recursive: true });
  fs.mkdirSync(home, { recursive: true });
  return {
    ...process.env, HOME: home, npm_config_cache: npmCache, NPM_CONFIG_CACHE: npmCache,
    npm_config_audit: "false", npm_config_fund: "false", npm_config_update_notifier: "false",
  };
}

export function makeFixtureArchives({ fixtureRoot, root }) {
  const packagesDir = path.join(fixtureRoot, "packages");
  fs.mkdirSync(packagesDir, { recursive: true });
  const nativeSource = path.join(fixtureRoot, "fixture.c");
  const nativeBinary = path.join(fixtureRoot, "fixture-native");
  fs.writeFileSync(nativeSource, `#include <stdio.h>\n#include <string.h>\nint main(int argc, char **argv) {\n  if (argc == 2 && strcmp(argv[1], "--version") == 0) puts("hardgate 0.5.0 (aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa)");\n  return 0;\n}\n`);
  run("cc", ["-O2", nativeSource, "-o", nativeBinary]);
  fs.chmodSync(nativeBinary, 0o755);
  const names = [
    "hardgate-linux-x64", "hardgate-linux-x64-musl", "hardgate-linux-arm64",
    "hardgate-linux-arm64-musl", "hardgate-darwin-x64", "hardgate-darwin-arm64",
  ];
  for (const name of ["hardgate", ...names]) {
    const packageDirectory = path.join(fixtureRoot, name);
    fs.cpSync(path.join(root, "npm", name), packageDirectory, { recursive: true });
    // This archive fixture deliberately represents a historical release;
    // keep it independent of the next source version under development.
    const manifestPath = path.join(packageDirectory, "package.json");
    const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
    manifest.version = "0.5.0";
    for (const dependency of Object.keys(manifest.optionalDependencies ?? {})) manifest.optionalDependencies[dependency] = "0.5.0";
    fs.writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + "\n");
    if (name !== "hardgate") {
      fs.copyFileSync(nativeBinary, path.join(packageDirectory, "bin", "hardgate"));
      fs.chmodSync(path.join(packageDirectory, "bin", "hardgate"), 0o755);
    }
    run("npm", ["pack", "--json", "--loglevel=error", "--pack-destination", packagesDir], {
      cwd: packageDirectory, env: fixtureEnvironment(fixtureRoot),
    });
  }
  return { packagesDir, nativeBinary };
}

export function packModified({ fixtureRoot }, { sourceName, label, mutate, mutateFiles = () => {} }) {
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
    cwd: packageDirectory, env: fixtureEnvironment(fixtureRoot),
  });
  const modified = new Set(fs.readdirSync(packagesDir).filter((name) => name.endsWith(".tgz")));
  for (const archive of fs.readdirSync(path.join(fixtureRoot, "packages")).filter((name) => name.endsWith(".tgz"))) {
    if (!modified.has(archive)) fs.copyFileSync(path.join(fixtureRoot, "packages", archive), path.join(packagesDir, archive));
  }
  return packagesDir;
}

export function makeDuplicateManifestArchives({ fixtureRoot }) {
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

export function makeUnsafeArchive({ fixtureRoot }) {
  const directory = path.join(fixtureRoot, "unsafe-member");
  fs.mkdirSync(directory, { recursive: true });
  const source = fs.readdirSync(path.join(fixtureRoot, "packages")).find((name) => name.endsWith(".tgz"));
  assert.ok(source, "fixture archive must be present");
  const tar = zlib.gunzipSync(fs.readFileSync(path.join(fixtureRoot, "packages", source)));
  tar.fill(0, 0, 100);
  Buffer.from("package//unsafe").copy(tar, 0);
  fs.writeFileSync(path.join(directory, "unsafe.tgz"), zlib.gzipSync(tar));
  return directory;
}

export function httpStatus(url) {
  return new Promise((resolve, reject) => {
    const request = http.get(url, (response) => {
      response.resume();
      response.once("end", () => resolve(response.statusCode));
    });
    request.once("error", reject);
  });
}

export async function makeSkippedOptionalRoot({ fixtureRoot, nativeBinary, registry }) {
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
    ...fixtureEnvironment(fixtureRoot), HOME: home, NPM_CONFIG_CACHE: cache, npm_config_cache: cache,
    NPM_CONFIG_USERCONFIG: userConfig, npm_config_userconfig: userConfig,
    NPM_CONFIG_REGISTRY: registry.baseUrl, npm_config_registry: registry.baseUrl,
    NPM_CONFIG_OMIT: "optional", npm_config_omit: "optional", NPM_CONFIG_INCLUDE: "", npm_config_include: "",
    NPM_CONFIG_IGNORE_SCRIPTS: "false", npm_config_ignore_scripts: "false", HTTP_PROXY: "", HTTPS_PROXY: "",
    ALL_PROXY: "", http_proxy: "", https_proxy: "", all_proxy: "", NPM_CONFIG_PROXY: "", npm_config_proxy: "",
    NPM_CONFIG_HTTPS_PROXY: "", npm_config_https_proxy: "",
  };
  fs.mkdirSync(cache, { recursive: true });
  fs.mkdirSync(home, { recursive: true });
  fs.writeFileSync(userConfig, `registry=${registry.baseUrl}\ncache=${cache}\nomit=optional\nignore-scripts=false\naudit=false\nfund=false\n`);
  await runAsync("npm", ["install", "--no-audit", "--no-fund", "--omit=optional", "--registry", registry.baseUrl, "@tech-byte-frontier/hardgate@0.5.0"], { cwd: root, env });
  fs.copyFileSync(nativeBinary, path.join(ambient, "hardgate"));
  fs.chmodSync(path.join(ambient, "hardgate"), 0o755);
  fs.writeFileSync(path.join(sentinel, "hardgate"), "#!/bin/sh\nexit 127\n", { mode: 0o755 });
  return {
    root, launcher: path.join(root, "node_modules", "@tech-byte-frontier", "hardgate", "bin", "hardgate.js"),
    environment: { PATH: [sentinel, ambient, path.dirname(process.execPath), "/usr/bin", "/bin"].join(path.delimiter) },
    ambient: path.join(ambient, "hardgate"),
  };
}
