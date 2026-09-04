// Static contract for release safety. This test deliberately avoids a YAML
// dependency so it can run before any package installation or publication.
"use strict";

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (relative) => fs.readFileSync(path.join(root, relative), "utf8");
const ci = read(".github/workflows/ci.yml");
const release = read(".github/workflows/release.yml");
const installer = read("scripts/install.sh");
const packageScript = read("scripts/release-package.mjs");
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
    assert.match(line, /#\s*v?[0-9]/i, `${label} pin needs a version comment: ${line.trim()}`);
  }
}

includesAll(ci, [
  "cargo fmt --all --check",
  "cargo clippy --all-targets --all-features --locked -- -D warnings",
  "cargo test --all-targets --all-features --locked",
  "scripts/dependency-audit.sh",
  "scripts/self-gate.sh",
  "CARGO_AUDIT_VERSION: 0.21.2",
  "CARGO_LLVM_COV_VERSION: 0.9.0",
  "components: rustfmt, clippy, llvm-tools-preview",
  "node scripts/check-npm-quality.mjs",
  "node tests/npm-wrapper.test.mjs",
  "node tests/npm-wrapper-regression.test.mjs",
  "node scripts/check-consumer-matrix.mjs",
  "HARDGATE_BINARY: target/release/hardgate",
], "CI");
includesAll(release, [
  "CARGO_AUDIT_VERSION: 0.21.2",
  "CARGO_LLVM_COV_VERSION: 0.9.0",
  "cargo fmt --all --check",
  "cargo clippy --all-targets --all-features --locked -- -D warnings",
  "cargo test --all-targets --all-features --locked",
  "scripts/dependency-audit.sh",
  "scripts/self-gate.sh",
  "node scripts/check-npm-quality.mjs",
  "node tests/npm-wrapper.test.mjs",
  "node tests/npm-wrapper-regression.test.mjs",
  "node scripts/check-consumer-matrix.mjs",
  "HARDGATE_BINARY: target/release/hardgate",
  "fail-fast: true",
  "SOURCE_DATE_EPOCH=0",
  "SHA256SUMS",
  "sha256sum --check --strict",
  "scripts/release-package.mjs",
  "scripts/release-verify.mjs",
  "scripts/release-sbom.mjs",
  "scripts/sync-npm-version.mjs --check --tag",
  "cargo publish --locked",
  "https://crates.io/api/v1/crates/hardgate/",
  'version"]["num',
  "actions/attest",
  "publish-crates",
  "verify-channels:",
], "release");
includesAll(packageScript, ["--sort=name", "--mtime=@0", "gzip", "-n", "SHA256SUMS", "chmodSync(destination, 0o755)", "full hexadecimal source identity"], "archive helper");
includesAll(coverageScript, ["CARGO_LLVM_COV_VERSION", "cargo install cargo-llvm-cov --version", "--all-targets", "--all-features", "--lcov", "coverage/lcov.info"], "coverage helper");
includesAll(auditScript, ["CARGO_AUDIT_VERSION", "cargo install cargo-audit --version", "cargo audit"], "audit helper");
includesAll(selfGate, ["check --all --dead-code --format agent", "verify --coverage-report coverage/lcov.info --format agent", "mutate", "--max-mutants 1", "cargo test --all-targets --all-features --locked", "HARDGATE_BINARY=\"$BINARY\" node scripts/check-consumer-matrix.mjs"], "self gate");
assert.doesNotMatch(selfGate, /verify --format agent\b/, "self gate must not claim complete evidence after disabling evidence engines");

for (const target of targets) assert.ok(release.includes(target), `release must build ${target}`);
for (const packageName of platformPackages) {
  assert.ok(release.includes(packageName), `release must handle ${packageName}`);
}
assert.deepEqual(npmPlatformDirectories, [...platformPackages].sort(), "npm directories must match the supported platform set");
assert.deepEqual(Object.keys(wrapperManifest.optionalDependencies ?? {}).sort(), [...platformPackages].sort(), "wrapper optionalDependencies must match the supported platform set");
includesAll(installer, ["linux-x86_64", "linux-aarch64|linux-arm64", "darwin-x86_64", "darwin-aarch64|darwin-arm64", "libc_suffix"], "installer platform map");
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
assert.match(cargo, /installers\s*=\s*\[\s*"shell"\s*\]/);

includesAll(installer, [
  "HARDGATE_VERSION",
  "vX.Y.Z",
  "SHA256SUMS",
  "sha256sum --check",
  "latest/download",
  "releases/download",
  "archive metadata has no full source commit identity",
], "installer");
includesAll(build, [
  "HARDGATE_BUILD_GIT_SHA",
  ".cargo_vcs_info.json",
  "git",
  "rev-parse",
  '"unknown"',
], "build identity");
includesAll(buildInfo, ["CARGO_PKG_VERSION", "HARDGATE_BUILD_GIT_SHA", "VERSION_DISPLAY"], "version display");

console.log("release_contract.test: OK");
