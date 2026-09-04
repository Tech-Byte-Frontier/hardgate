// Static contract for release safety. This test deliberately avoids a YAML
// dependency so it can run before any package installation or publication.
"use strict";

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { isRetryableNpmPackError } from "../scripts/npm-pack-retry.mjs";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (relative) => fs.readFileSync(path.join(root, relative), "utf8");
const ci = read(".github/workflows/ci.yml");
const release = read(".github/workflows/release.yml");
const installer = read("scripts/install.sh");
const packageScript = read("scripts/release-package.mjs");
const checksumScript = read("scripts/release-checksums.mjs");
const verifier = read("scripts/release-verify.mjs");
const npmPublication = read("scripts/verify-npm-publication.mjs");
const npmPackRetry = read("scripts/npm-pack-retry.mjs");
const launcher = read("npm/hardgate/bin/hardgate.js");
const sbomScript = read("scripts/release-sbom.mjs");
const sbomVerifier = read("scripts/release-sbom-verify.mjs");
const syncScript = read("scripts/sync-npm-version.mjs");
const installerRuntime = read("tests/release_contract.install.test.mjs");
const coverageScript = read("scripts/coverage.sh");
const auditScript = read("scripts/dependency-audit.sh");
const selfGate = read("scripts/self-gate.sh");
const cargo = read("Cargo.toml");
const build = read("build.rs");
const buildInfo = read("src/build_info.rs");

const platformPackages = [
  "hardgate-linux-x64",
  "hardgate-linux-x64-musl",
  "hardgate-linux-arm64",
  "hardgate-linux-arm64-musl",
  "hardgate-darwin-x64",
  "hardgate-darwin-arm64",
];
const targets = [
  "x86_64-unknown-linux-gnu",
  "x86_64-unknown-linux-musl",
  "aarch64-unknown-linux-gnu",
  "aarch64-unknown-linux-musl",
  "x86_64-apple-darwin",
  "aarch64-apple-darwin",
];

const npmRoot = path.join(root, "npm");
const npmPlatformDirectories = fs
  .readdirSync(npmRoot, { withFileTypes: true })
  .filter((entry) => entry.isDirectory() && entry.name !== "hardgate")
  .map((entry) => entry.name)
  .sort();
const wrapperManifest = JSON.parse(fs.readFileSync(path.join(npmRoot, "hardgate/package.json"), "utf8"));

function includesAll(text, snippets, label) {
  for (const snippet of snippets) assert.ok(text.includes(snippet), `${label} must contain ${snippet}`);
}

// Every third-party action is immutable and carries a human-readable release
// comment. A floating branch/tag is a supply-chain regression.
for (const [label, text] of [["CI", ci], ["release", release]]) {
  for (const line of text.split("\n").filter((value) => value.includes("uses:"))) {
    assert.match(line, /@[0-9a-f]{40}\b/i, `${label} action is not pinned: ${line.trim()}`);
    assert.match(line, /#\s*(?:v?[0-9]|master\b)/i, `${label} pin needs a version comment: ${line.trim()}`);
  }
}

includesAll(ci, [
  "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
  "dtolnay/rust-toolchain@d1031067263f94b142dd6c0ce24c5eb9d02d52a0",
  "pnpm/setup@703c52620218391530e48b9e8870d5c0082e1b9b",
  "oven-sh/setup-bun@0c5077e51419868618aeaa5fe8019c62421857d6",
  "cargo fmt --all --check",
  "cargo clippy --all-targets --all-features --locked -- -D warnings",
  "cargo test --all-targets --all-features --locked",
  "scripts/dependency-audit.sh",
  "scripts/self-gate.sh",
  "CARGO_AUDIT_VERSION: 0.22.2",
  "CARGO_LLVM_COV_VERSION: 0.9.0",
  "NODE_VERSION: 26.8.1",
  "NPM_VERSION: 12.0.2",
  "PNPM_VERSION: 11.25.0",
  "YARN_VERSION: 4.18.0",
  "BUN_VERSION: 1.4.0",
  "components: rustfmt, clippy, llvm-tools-preview",
  "node scripts/check-npm-quality.mjs",
  "node tests/npm-wrapper.test.mjs",
  "node tests/npm-wrapper-regression.test.mjs",
  "node tests/release_contract.install.test.mjs",
  "node tests/release_contract.package.test.mjs",
  "node scripts/check-consumer-matrix.mjs",
  "HARDGATE_BINARY: target/release/hardgate",
], "CI");
includesAll(release, [
  "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
  "dtolnay/rust-toolchain@d1031067263f94b142dd6c0ce24c5eb9d02d52a0",
  "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a",
  "actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c",
  "pnpm/setup@703c52620218391530e48b9e8870d5c0082e1b9b",
  "oven-sh/setup-bun@0c5077e51419868618aeaa5fe8019c62421857d6",
  "actions/attest@1e69f48acb82d1966a394da916b4c1698aa569d6",
  "CARGO_AUDIT_VERSION: 0.22.2",
  "CARGO_LLVM_COV_VERSION: 0.9.0",
  "NODE_VERSION: 26.8.1",
  "NPM_VERSION: 12.0.2",
  "PNPM_VERSION: 11.25.0",
  "YARN_VERSION: 4.18.0",
  "BUN_VERSION: 1.4.0",
  "cargo fmt --all --check",
  "cargo clippy --all-targets --all-features --locked -- -D warnings",
  "cargo test --all-targets --all-features --locked",
  "scripts/dependency-audit.sh",
  "scripts/self-gate.sh",
  "node scripts/check-npm-quality.mjs",
  "node tests/npm-wrapper.test.mjs",
  "node tests/npm-wrapper-regression.test.mjs",
  "node tests/release_contract.install.test.mjs",
  "node scripts/check-consumer-matrix.mjs",
  "HARDGATE_BINARY: target/release/hardgate",
  "git cat-file -t \"$RELEASE_TAG\"",
  "fail-fast: true",
  "SOURCE_DATE_EPOCH=0",
  "SHA256SUMS",
  "sha256sum --check --strict",
  "scripts/release-package.mjs",
  "scripts/release-checksums.mjs",
  "scripts/release-verify.mjs",
  "scripts/verify-npm-publication.mjs",
  "scripts/release-sbom.mjs",
  "scripts/release-sbom-verify.mjs",
  "scripts/sync-npm-version.mjs --check --tag",
  "cargo publish --locked",
  "https://crates.io/api/v1/crates/hardgate/",
  'version"]["num',
  "actions/attest",
  "publish-crates",
  "verify-channels:",
  "hardgate-${RELEASE_VERSION}.sbom.cdx.json",
  "already_published=0",
  "crate_probe()",
  "crate_version()",
  "npm_registry_probe()",
  "wait_for_registry_version()",
  "wait_for_crate_version()",
  'publish_token="${NODE_AUTH_TOKEN:?NPM_TOKEN is required for npm publication}"',
  'unset NODE_AUTH_TOKEN',
  'NODE_AUTH_TOKEN="$publish_token" npm publish --provenance --access public',
  "return 2",
  "404)",
  "crates.io version probe failed; refusing to publish",
  "npm registry version probe failed",
  "gh release download",
  "cmp --",
  "wait_for_registry_version 1",
  "wait_for_crate_version 1",
  "cargo install hardgate --version \"=$RELEASE_VERSION\"",
  "npm install --ignore-scripts",
  "--package \"$pkg\"",
  "env -u NODE_AUTH_TOKEN node scripts/verify-npm-publication.mjs",
  "Verify clean npm, pnpm, Yarn, and Bun consumers",
  "pnpm add --ignore-scripts",
  "yarn add",
  "bun add",
  "HARDGATE_INSTALL_DIR=\"$install_root\" sh scripts/install.sh",
  "HARDGATE_CURL_CONNECT_TIMEOUT: 10",
  "HARDGATE_CURL_MAX_TIME: 20",
  "HARDGATE_REGISTRY_ATTEMPTS: 10",
  "HARDGATE_REGISTRY_DELAY: 10",
  "release_error=$(mktemp)",
  "release_exists=0",
  "unable to determine whether GitHub release",
], "release");
assert.doesNotMatch(release, /--clobber/, "immutable release assets must never be overwritten in place");
assert.doesNotMatch(release, /npm view/, "final registry verification must use status-aware probes");
assert.doesNotMatch(release, /if gh release view \"\$RELEASE_TAG\"(?: --json tagName)? >\/dev\/null 2>&1/, "release creation must distinguish not-found from API failures");
assert.doesNotMatch(release, /^[ \t]*registry_version\(\)/m, "registry waits must not multiply nested retry loops");
includesAll(release, ["retry_absent", "return 3", "release_error=$(mktemp)", "release_exists=0", "refusing to create or mutate it"], "status-aware release waits");
const registryAttempts = Number(release.match(/HARDGATE_REGISTRY_ATTEMPTS:\s*(\d+)/)?.[1]);
const registryDelay = Number(release.match(/HARDGATE_REGISTRY_DELAY:\s*(\d+)/)?.[1]);
const curlMaxTime = Number(release.match(/HARDGATE_CURL_MAX_TIME:\s*(\d+)/)?.[1]);
assert.ok(Number.isInteger(registryAttempts) && Number.isInteger(registryDelay) && Number.isInteger(curlMaxTime));
assert.ok(registryAttempts * curlMaxTime + (registryAttempts - 1) * registryDelay <= 300, "each registry wait must fit within five minutes");
assert.doesNotMatch(release, /macos-14/, "deprecated macos-14 runners must not be launched");
assert.doesNotMatch(
  launcher,
  /\["win32"\s*,|hardgate-win32|\.exe\b|\bMZ\b|homebrew|\bbrew\b/i,
  "published launcher must not advertise or package unsupported Windows or Homebrew channels",
);
includesAll(launcher, ["function detectMusl", "glibcVersionRuntime", "trim().length", "static musl package"], "generic Linux libc detection");
assert.doesNotMatch(launcher, /hasAlpineRelease|alpineReleaseExists/, "Linux libc detection must not depend on an Alpine-only marker");
includesAll(
  launcher,
  [
    "function exitFromSpawn",
    "result.status ?? 1",
    'process.platform !== "win32"',
    "process.kill(process.pid, result.signal)",
    "process.exit(1)",
  ],
  "launcher signal handling",
);
assert.match(release, /permissions:\s*\n\s+contents: read/, "release workflow default token must be read-only");
assert.match(release, /package:[\s\S]*?permissions:[\s\S]*?attestations: write/, "only packaging may attest artifacts");
assert.match(release, /github-release:[\s\S]*?permissions:[\s\S]*?contents: write/, "only GitHub publication may write contents");
assert.match(release, /publish-npm:[\s\S]*?permissions:[\s\S]*?id-token: write/, "npm provenance publication requires scoped OIDC access");
const cratePublishStep = release.slice(release.indexOf("- name: Publish crate when exact version is missing"), release.indexOf("- name: Verify published crate identity without publish credentials"));
const crateVerifyStep = release.slice(release.indexOf("- name: Verify published crate identity without publish credentials"), release.indexOf("  publish-npm:"));
assert.match(cratePublishStep, /CARGO_REGISTRY_TOKEN:[\s\S]*?cargo publish --locked/, "crate token must scope only publication");
assert.doesNotMatch(crateVerifyStep, /CARGO_REGISTRY_TOKEN/, "crate verification must not inherit publish credentials");
for (const job of ["version-check", "package", "github-release", "publish-crates", "publish-npm", "verify-channels"]) {
  assert.match(release, new RegExp(`${job}:[\\s\\S]*?runs-on: ubuntu-24\\.04`), `${job} should use the current x64 Linux runner`);
}
assert.equal((ci.match(/actions\/checkout@/g) ?? []).length, (ci.match(/persist-credentials: false/g) ?? []).length, "CI checkouts must not persist GitHub credentials");
assert.equal((release.match(/actions\/checkout@/g) ?? []).length, (release.match(/persist-credentials: false/g) ?? []).length, "release checkouts must not persist GitHub credentials");
includesAll(packageScript, ["--sort=name", "--mtime=@0", "gzip", "-n", "SHA256SUMS", "chmodSync(packageRoot, 0o755)", "chmodSync(destination, 0o755)", "metadataPath", "chmodSync(metadataPath, 0o644)", "full hexadecimal source identity"], "archive helper");
includesAll(checksumScript, ["SHA256SUMS", "hardgate-${version}.sbom.cdx.json", "lines.length", "sha256"], "payload checksum helper");
includesAll(syncScript, ["syncJson(path.join(root, \"package.json\")", "--check", "Cargo.toml"], "version synchronization");
includesAll(verifier, ["MAX_BINARY_BYTES", "verifyEmbeddedIdentity", "verifyExecutableMember", "tar", "-tvzf", "fs.chmodSync(binaryPath, 0o755)", "Buffer.from(`${version} (${commit})`", "expectedOutput", "result.stdout.trim() !== expectedOutput", "verifyBinaryAbi", "readelf", "ld-musl", "ld-linux", "static(?:-pie)?", "muslInterpreter"], "archive verifier");
includesAll(npmPublication, ["--platform-only", "--package", "npm pack", "optionalDependencies", "byte-match", "path.join(packageDirectory, \"bin/hardgate\")", "tar", "-tvzf", "npm/hardgate/bin/hardgate.js", "NPM_VERIFY_ATTEMPTS", "isRetryableNpmPackError", "failed without retry"], "npm publication verifier");
includesAll(npmPackRetry, ["isRetryableNpmPackError", "E404", "EAI_AGAIN", "ECONNRESET", "ETIMEDOUT", "ECONNREFUSED"], "npm pack retry classifier");
for (const error of [
  { code: "E404" },
  { message: "npm ERR! HTTP 404" },
  { code: "EAI_AGAIN" },
  { code: "ECONNRESET" },
  { code: "ETIMEDOUT" },
  { code: "ECONNREFUSED" },
]) assert.equal(isRetryableNpmPackError(error), true, `expected retryable npm pack error: ${JSON.stringify(error)}`);
for (const error of [
  { code: "E401" },
  { code: "E403" },
  { message: "npm pack produced 0 tarballs" },
  { message: "npm pack exited with status 1" },
]) assert.equal(isRetryableNpmPackError(error), false, `expected fatal npm pack error: ${JSON.stringify(error)}`);
assert.doesNotMatch(verifier, /startsWith\(`hardgate \$\{version\}/, "host smoke must compare the complete identity");
assert.match(sbomScript, /expression: licenseText/, "CycloneDX must encode compound SPDX values as expressions");
assert.match(sbomScript, /id: licenseText/, "CycloneDX may encode a single SPDX identifier as an id");
includesAll(sbomScript, ["$schema", "rootComponent.type = \"application\"", "components.filter", "codepointCompare", "Buffer.from(left, \"utf8\").compare"], "CycloneDX structure");
assert.doesNotMatch(sbomScript, /id:\s*pkg\.license/, "raw package SPDX expressions must not be emitted as license ids");
includesAll(sbomVerifier, ["CycloneDX", "1.5", "metadata.component", "application", "must not be duplicated", "license.expression", "$schema"], "CycloneDX verifier");
includesAll(coverageScript, ["CARGO_LLVM_COV_VERSION", "0.9.0", "cargo install cargo-llvm-cov --version \"=$COV_VERSION\"", "--all-targets", "--all-features", "--lcov", "coverage/lcov.info"], "coverage helper");
includesAll(auditScript, ["CARGO_AUDIT_VERSION", "0.22.2", "cargo install cargo-audit --version \"=$AUDIT_VERSION\"", "cargo audit"], "audit helper");
includesAll(selfGate, ["check --all --dead-code --format agent", "verify --coverage-report coverage/lcov.info --format agent", "mutate", "--max-mutants 1", "cargo test --all-targets --all-features --locked", "HARDGATE_BINARY=\"$BINARY\" node scripts/check-consumer-matrix.mjs"], "self gate");
assert.doesNotMatch(selfGate, /verify --format agent\b/, "self gate must not claim complete evidence after disabling evidence engines");

for (const target of targets) assert.ok(release.includes(target), `release must build ${target}`);
for (const packageName of platformPackages) {
  assert.ok(release.includes(packageName), `release must handle ${packageName}`);
}
assert.deepEqual(npmPlatformDirectories, [...platformPackages].sort(), "npm directories must match the supported platform set");
assert.deepEqual(Object.keys(wrapperManifest.optionalDependencies ?? {}).sort(), [...platformPackages].sort(), "wrapper optionalDependencies must match the supported platform set");
includesAll(installer, ["linux-x86_64", "linux-aarch64|linux-arm64", "darwin-x86_64", "darwin-aarch64|darwin-arm64", "libc_suffix", "HARDGATE_LIBC=gnu|musl", "ldd --version", "ld-musl-*.so.1"], "installer platform map");
assert.equal((release.match(/target:/g) ?? []).length, targets.length, "release matrix must contain exactly six targets");

assert.match(release, /needs:\s*\[version-check, quality-gate\]/, "build must wait for quality");
assert.match(release, /package:[\s\S]*needs:\s*\[version-check, quality-gate, build\]/, "packaging must wait for all builds");
assert.match(release, /github-release:[\s\S]*needs:\s*\[version-check, quality-gate, package\]/, "GitHub publication must wait for verification");
assert.match(release, /publish-npm:[\s\S]*needs:\s*\[version-check, quality-gate, package, github-release, publish-crates\]/, "npm publication must wait for crate publication");
assert.match(release, /verify-channels:[\s\S]*needs:\s*\[version-check, github-release, publish-crates, publish-npm\]/, "final channel verification must wait for every publication");
const platformPublish = release.indexOf("Publish and verify each platform package in order");
const wrapperPublish = release.indexOf("Publish wrapper only after all platforms are verified");
assert.ok(platformPublish >= 0 && wrapperPublish > platformPublish, "wrapper publication must follow platform publication");
assert.doesNotMatch(release, /deliberately tolerant|continu(?:e|ing) with remaining|main wrapper above was still attempted/i);

assert.doesNotMatch(installer, /win32|windows|\.exe|powershell/i, "installer must advertise Unix targets only");
assert.doesNotMatch(release, /win32|windows|homebrew|brew/i, "release must not advertise removed channels");
assert.doesNotMatch(cargo, /homebrew|tap\s*=/i, "Cargo metadata must not advertise an unmaintained channel");
assert.doesNotMatch(cargo, /\[package\.metadata\.dist\]/, "hand-authored release workflow is authoritative");
assert.match(cargo, /rust-version\s*=\s*"1\.98\.1"/);

includesAll(installer, [
  "HARDGATE_VERSION",
  "HOME is required unless HARDGATE_INSTALL_DIR is set",
  "vX.Y.Z",
  "SHA256SUMS",
  "sha256sum \"$tmp/$archive_name\"",
  "latest/download",
  "releases/download",
  "archive metadata has no full source commit identity",
  "archive metadata has no valid release version",
  "metadata_package",
  "metadata_target",
  "archive members do not exactly match",
  "expected_members",
  "installed_name_version",
  "hardgate ${metadata_version} (${metadata_commit})",
  "mktemp -d \"$INSTALL_DIR/.hardgate.XXXXXX\"",
], "installer");
assert.doesNotMatch(installer, /sha256sum --check|sha256sum --status/, "installer checksum verification must work with BusyBox");
includesAll(installer, [
  "HARDGATE_CURL_CONNECT_TIMEOUT",
  "HARDGATE_CURL_MAX_TIME",
  "--connect-timeout \"$CURL_CONNECT_TIMEOUT\"",
  "--max-time \"$CURL_MAX_TIME\"",
  "destination=\"$INSTALL_DIR/hardgate\"",
  "destination ${destination} is a directory",
], "installer safety and bounded downloads");
const installerCurlInvocations = installer.match(/\bcurl\s+--/g) ?? [];
assert.ok(installerCurlInvocations.length >= 2, "installer must have checksum and archive downloads");
assert.equal((installer.match(/--connect-timeout/g) ?? []).length, installerCurlInvocations.length, "every installer curl must set connect timeout");
assert.equal((installer.match(/--max-time/g) ?? []).length, installerCurlInvocations.length, "every installer curl must set max time");
const releaseCurlInvocations = release.match(/\bcurl\s+--/g) ?? [];
assert.ok(releaseCurlInvocations.length > 0, "release must probe registries with curl");
assert.equal((release.match(/--connect-timeout/g) ?? []).length, releaseCurlInvocations.length, "every release curl must set connect timeout");
assert.equal((release.match(/--max-time/g) ?? []).length, releaseCurlInvocations.length, "every release curl must set max time");
includesAll(installerRuntime, ["regular-file replacement", "existing destination directory", "symlink-to-directory", "is a directory", "BusyBox"], "installer destination regression");
includesAll(installerRuntime, ["BusyBox", "sha256sum", "HARDGATE_FIXTURE", "ldd", "EXTRA.txt", "hardgate-linux-x64-musl", "release_contract.install.test"], "installer runtime contract");
includesAll(build, [
  "HARDGATE_BUILD_GIT_SHA",
  ".cargo_vcs_info.json",
  "git",
  "rev-parse",
  '"unknown"',
], "build identity");
includesAll(buildInfo, ["CARGO_PKG_VERSION", "HARDGATE_BUILD_GIT_SHA", "VERSION_DISPLAY"], "version display");

console.log("release_contract.test: OK");
