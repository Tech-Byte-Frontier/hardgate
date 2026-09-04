// Static archive, installer, and build-identity assertions for release safety.
"use strict";

import assert from "node:assert/strict";
import { isRetryableNpmPackError } from "../scripts/npm-pack-retry.mjs";
import {
  auditScript,
  build,
  buildInfo,
  cargo,
  checksumScript,
  ci,
  coverageScript,
  includesAll,
  installer,
  installerRuntime,
  nodeVersion,
  npmPackRetry,
  npmPlatformDirectories,
  npmPublication,
  packageScript,
  platformPackages,
  release,
  releaseAbi,
  rustToolchain,
  sbomScript,
  sbomVerifier,
  selfGate,
  syncScript,
  targets,
  verifier,
  wrapperManifest,
} from "./release_contract.sources.mjs";

includesAll(packageScript, ["--sort=name", "--mtime=@0", "gzip", "-n", "SHA256SUMS", "chmodSync(packageRoot, 0o755)", "chmodSync(destination, 0o755)", "metadataPath", "chmodSync(metadataPath, 0o644)", "full hexadecimal source identity"], "archive helper");
includesAll(checksumScript, ["SHA256SUMS", "hardgate-${version}.sbom.cdx.json", "lines.length", "sha256"], "payload checksum helper");
includesAll(syncScript, ["syncJson(path.join(root, \"package.json\")", "--check", "Cargo.toml"], "version synchronization");
const cargoInclude = cargo.match(/^include\s*=\s*\[([\s\S]*?)^\]/m);
assert.ok(cargoInclude, "Cargo package must have an explicit root-anchored include allowlist");
const cargoIncludeEntries = [...cargoInclude[1].matchAll(/"([^"]+)"/g)].map((match) => match[1]);
assert.deepEqual(
  cargoIncludeEntries,
  [
    "/Cargo.toml",
    "/Cargo.lock",
    "/build.rs",
    "/src/**",
    "/tests/**/*.rs",
    "/tests/common/*.txt",
    "/README.md",
    "/CHANGELOG.md",
    "/LICENSE-MIT",
    "/LICENSE-APACHE",
    "/docs/ARCHITECTURE.md",
    "/docs/CLI_AND_INTEGRATION.md",
    "/docs/CONFIGURATION_SPEC.md",
    "/docs/EXISTING_LANDSCAPE.md",
    "/docs/VISION_AND_PARADIGM.md",
  ],
  "Cargo package allowlist must exclude workspace, generated, release, and private local artifacts",
);
includesAll(verifier, ["MAX_BINARY_BYTES", "verifyEmbeddedIdentity", "verifyExecutableMember", "tar", "-tvzf", "fs.chmodSync(binaryPath, 0o755)", "Buffer.from(`${version} (${commit})`", "hardgate-target:", "expected Cargo target marker", "expectedOutput", "result.stdout.trim() !== expectedOutput", "verifyBinaryAbi", "readelf", "-l", "-sW", "-n", "classifyBinaryAbi"], "archive verifier");
includesAll(releaseAbi, ["classifyBinaryAbi", "ld-musl", "__init_libc", "GLIBC_", "gnu_get_libc_version", "_dl_relocate_static_pie", "NT_GNU_ABI_TAG", "staticBinary", "exact Cargo target marker", "targetMarkerValid"], "ABI evidence classifier");
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
includesAll(coverageScript, ["CARGO_LLVM_COV_VERSION", "COV_TOOLCHAIN=\"${RUST_COVERAGE_TOOLCHAIN:-nightly-2026-09-04}\"", "0.9.0", "cargo install cargo-llvm-cov --version \"=$COV_VERSION\"", "cargo \"+$COV_TOOLCHAIN\" llvm-cov --version", "--all-targets", "--all-features", "--branch", "--include-build-script", "--lcov", "coverage/lcov.info"], "coverage helper");
includesAll(auditScript, ["CARGO_AUDIT_VERSION", "0.22.2", "cargo install cargo-audit --version \"=$AUDIT_VERSION\"", "cargo audit"], "audit helper");
includesAll(selfGate, ["check --all --dead-code --format agent", "verify --coverage-report coverage/lcov.info --format agent", "mutate", "--max-mutants 1", "cargo test --all-targets --all-features --locked", "HARDGATE_BINARY=\"$BINARY\" node scripts/check-consumer-matrix.mjs"], "self gate");
assert.doesNotMatch(selfGate, /verify --format agent\b/, "self gate must not claim complete evidence after disabling evidence engines");

for (const target of targets) assert.ok(release.includes(target), `release must build ${target}`);
for (const packageName of platformPackages) {
  assert.ok(release.includes(packageName), `release must handle ${packageName}`);
}
assert.deepEqual(npmPlatformDirectories, [...platformPackages].sort(), "npm directories must match the supported platform set");
assert.deepEqual(Object.keys(wrapperManifest.optionalDependencies ?? {}).sort(), [...platformPackages].sort(), "wrapper optionalDependencies must match the supported platform set");
includesAll(installer, ["linux-x86_64", "linux-aarch64|linux-arm64", "darwin-x86_64", "darwin-aarch64|darwin-arm64", "libc_suffix", "gnu|glibc)", "musl)", "HARDGATE_LIBC must be gnu, glibc, or musl", "ldd --version", "ld-musl-*.so.1"], "installer platform map");
assert.equal((release.match(/target:/g) ?? []).length, targets.length, "release matrix must contain exactly six targets");

assert.match(release, /needs:\s*\[version-check, quality-gate\]/, "build must wait for quality");
assert.match(release, /package:[\s\S]*needs:\s*\[version-check, quality-gate, build\]/, "packaging must wait for all builds");
assert.match(release, /github-release:[\s\S]*needs:\s*\[version-check, quality-gate, package, publication-preflight\]/, "GitHub publication must wait for verification and registry preflight");
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
includesAll(
  rustToolchain,
  ['channel = "1.98.1"', 'profile = "minimal"', 'components = ["clippy", "rustfmt", "llvm-tools-preview"]'],
  "repository Rust toolchain pin",
);
assert.equal(nodeVersion, "26.8.1", "repository Node pin must match the release toolchain");
assert.match(ci, new RegExp(`NODE_VERSION: ${nodeVersion.replaceAll(".", "\\.")}`));
assert.match(release, new RegExp(`NODE_VERSION: ${nodeVersion.replaceAll(".", "\\.")}`));

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
  "HARDGATE_BUILD_TARGET",
  'env::var("TARGET")',
  ".cargo_vcs_info.json",
  "git",
  "rev-parse",
  '"unknown"',
], "build identity");
includesAll(buildInfo, ["CARGO_PKG_VERSION", "HARDGATE_BUILD_GIT_SHA", "HARDGATE_BUILD_TARGET", "BUILD_TARGET_MARKER", "VERSION_DISPLAY"], "version display");
