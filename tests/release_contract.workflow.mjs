// Static workflow and publication-state assertions for release safety.
"use strict";

import assert from "node:assert/strict";
import {
  ci,
  includesAll,
  launcher,
  platformPackages,
  release,
} from "./release_contract.sources.mjs";

function assertWorkflowIncludes(text, label, snippets) {
  includesAll(text, snippets.trim().split(/\r?\n/).map((snippet) => snippet.trim()), label);
}

// Every third-party action is immutable and carries a human-readable release
// comment. A floating branch/tag is a supply-chain regression.
for (const [label, text] of [["CI", ci], ["release", release]]) {
  for (const line of text.split("\n").filter((value) => value.includes("uses:"))) {
    assert.match(line, /@[0-9a-f]{40}\b/i, `${label} action is not pinned: ${line.trim()}`);
    assert.match(line, /#\s*(?:v?[0-9]|master\b)/i, `${label} pin needs a version comment: ${line.trim()}`);
  }
}

assertWorkflowIncludes(
  ci,
  "CI",
  `
actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
dtolnay/rust-toolchain@d1031067263f94b142dd6c0ce24c5eb9d02d52a0
pnpm/setup@703c52620218391530e48b9e8870d5c0082e1b9b
oven-sh/setup-bun@0c5077e51419868618aeaa5fe8019c62421857d6
cargo fmt --all --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
scripts/dependency-audit.sh
scripts/self-gate.sh
CARGO_AUDIT_VERSION: 0.22.2
CARGO_LLVM_COV_VERSION: 0.9.0
RUST_COVERAGE_TOOLCHAIN: nightly-2026-09-04
NODE_VERSION: 26.8.1
NPM_VERSION: 12.0.2
PNPM_VERSION: 11.25.0
YARN_VERSION: 4.18.0
BUN_VERSION: 1.4.0
components: rustfmt, clippy, llvm-tools-preview
node scripts/check-npm-quality.mjs
node tests/npm-wrapper.test.mjs
node tests/npm-wrapper-regression.test.mjs
node tests/release_contract.install.test.mjs
node tests/release_contract.package.test.mjs
node tests/release_contract.abi.test.mjs
node tests/consumer_matrix.mjs
HARDGATE_BINARY: target/release/hardgate
`,
);
assertWorkflowIncludes(
  release,
  "release",
  `
actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
dtolnay/rust-toolchain@d1031067263f94b142dd6c0ce24c5eb9d02d52a0
actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a
actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c
pnpm/setup@703c52620218391530e48b9e8870d5c0082e1b9b
oven-sh/setup-bun@0c5077e51419868618aeaa5fe8019c62421857d6
actions/attest@1e69f48acb82d1966a394da916b4c1698aa569d6
actions: read
group: hardgate-release
CARGO_AUDIT_VERSION: 0.22.2
CARGO_LLVM_COV_VERSION: 0.9.0
NODE_VERSION: 26.8.1
NPM_VERSION: 12.0.2
PNPM_VERSION: 11.25.0
YARN_VERSION: 4.18.0
BUN_VERSION: 1.4.0
cargo fmt --all --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
scripts/dependency-audit.sh
scripts/self-gate.sh
node scripts/check-npm-quality.mjs
RUST_COVERAGE_TOOLCHAIN: nightly-2026-09-04
node tests/npm-wrapper.test.mjs
node tests/npm-wrapper-regression.test.mjs
node tests/release_contract.install.test.mjs
node tests/release_contract.package.test.mjs
node tests/release_contract.abi.test.mjs
node tests/consumer_matrix.mjs
HARDGATE_BINARY: target/release/hardgate
git cat-file -t "$RELEASE_TAG"
gpg.ssh.allowedSignersFile=.github/release-allowed-signers
verify-tag "$RELEASE_TAG"
refs/remotes/origin/main
actions/workflows/ci.yml/runs
release commit has no completed successful main CI run
fail-fast: true
SOURCE_DATE_EPOCH=0
SHA256SUMS
sha256sum --check --strict
scripts/release-package.mjs
scripts/release-checksums.mjs
scripts/release-verify.mjs
scripts/verify-npm-publication.mjs
scripts/release-sbom.mjs
scripts/release-sbom-verify.mjs
scripts/sync-npm-version.mjs --check --tag
cargo publish --locked
cargo publish --dry-run --locked
publication-preflight:
npm whoami --registry=https://registry.npmjs.org
needs: [version-check, quality-gate, package, publication-preflight]
https://crates.io/api/v1/crates/hardgate/
version"]["num
actions/attest
publish-crates
verify-channels:
hardgate-\${RELEASE_VERSION}.sbom.cdx.json
already_published=0
crate_probe()
crate_version()
npm_registry_probe()
wait_for_registry_version()
wait_for_crate_version()
publish_token="\${NODE_AUTH_TOKEN:?NPM_TOKEN is required for npm publication}"
unset NODE_AUTH_TOKEN
NODE_AUTH_TOKEN="$publish_token" npm publish --provenance --access public
return 2
404)
crates.io version probe failed; refusing to publish
npm registry version probe failed
gh release download
cmp --
wait_for_registry_version 1
wait_for_crate_version 1
cargo install hardgate --version "=$RELEASE_VERSION"
npm install --ignore-scripts
--package "$pkg"
env -u NODE_AUTH_TOKEN node scripts/verify-npm-publication.mjs
Verify clean npm, pnpm, Yarn, and Bun consumers
pnpm add --ignore-scripts
yarn add
bun add
HARDGATE_INSTALL_DIR="$install_root" sh scripts/install.sh
HARDGATE_CURL_CONNECT_TIMEOUT: 10
HARDGATE_CURL_MAX_TIME: 20
HARDGATE_REGISTRY_ATTEMPTS: 10
HARDGATE_REGISTRY_DELAY: 10
release_error=$(mktemp)
release_exists=0
unable to determine whether GitHub release
tagName,isDraft,isPrerelease
test "$release_is_draft" = false
test "$release_is_prerelease" = false
latest_release_tag=$(gh release view --json tagName --jq .tagName)
test "$latest_release_tag" = "$RELEASE_TAG"
npm_latest_probe()
wait_for_latest_tag()
["dist-tags"]["latest"]
`,
);

const ciSelfGate = ci.slice(ci.indexOf("  hardgate-self:"), ci.indexOf("  release-contract:"));
const releaseQualityGate = release.slice(release.indexOf("  quality-gate:"), release.indexOf("  build:"));
const crateDryRunStep = releaseQualityGate.slice(
  releaseQualityGate.indexOf("- name: Validate the crates.io package without publishing"),
  releaseQualityGate.indexOf("- run: node scripts/sync-npm-version.mjs"),
);
assert.match(crateDryRunStep, /cargo publish --dry-run --locked/, "release quality must validate the packaged crate");
assert.doesNotMatch(crateDryRunStep, /CARGO_REGISTRY_TOKEN|NODE_AUTH_TOKEN/, "package dry run must not expose publication credentials to build scripts");
for (const [label, section] of [["CI hardgate-self", ciSelfGate], ["release quality-gate", releaseQualityGate]]) {
  const coverageToolchain = section.indexOf("toolchain: ${{ env.RUST_COVERAGE_TOOLCHAIN }}");
  const stableToolchain = section.indexOf("toolchain: ${{ env.RUST_TOOLCHAIN }}");
  assert.ok(coverageToolchain >= 0, `${label} must install the pinned coverage toolchain`);
  assert.ok(stableToolchain > coverageToolchain, `${label} must leave stable Rust as the default toolchain`);
  assert.match(
    section.slice(coverageToolchain, stableToolchain),
    /components: llvm-tools-preview/,
    `${label} coverage toolchain must include llvm-tools-preview`,
  );
}

assert.doesNotMatch(release, /--clobber/, "immutable release assets must never be overwritten in place");
assert.doesNotMatch(release, /npm view/, "final registry verification must use status-aware probes");
assert.doesNotMatch(release, /https:\/\/crates\.io\/api\/v1\/me/, "crates.io /api/v1/me is cookie-only and cannot validate a publish token");
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
const finalReleaseBundleStep = release.slice(
  release.indexOf("- name: Download and verify the complete release bundle"),
  release.indexOf("- name: Verify published registry versions and runnable installs"),
);
includesAll(
  finalReleaseBundleStep,
  [
    "--json tagName,isDraft,isPrerelease",
    'test "$release_tag" = "$RELEASE_TAG"',
    'test "$release_is_draft" = false',
    'test "$release_is_prerelease" = false',
    "latest_release_tag=$(gh release view --json tagName --jq .tagName)",
    'test "$latest_release_tag" = "$RELEASE_TAG"',
  ],
  "current GitHub latest release guard",
);
const finalNpmVerificationStep = release.slice(
  release.indexOf("- name: Verify published registry versions and runnable installs"),
  release.indexOf("- name: Verify clean npm, pnpm, Yarn, and Bun consumers"),
);
includesAll(
  finalNpmVerificationStep,
  [
    "npm_latest_probe()",
    '"dist-tags"]["latest"]',
    "wait_for_latest_tag()",
    'wait_for_latest_tag "$pkg" "$RELEASE_VERSION"',
    'wait_for_latest_tag "@tech-byte-frontier/hardgate" "$RELEASE_VERSION"',
  ],
  "final npm latest dist-tag verification",
);
const finalNpmPlatformLoop = finalNpmVerificationStep.slice(
  finalNpmVerificationStep.indexOf("for pkg in hardgate-linux-x64"),
  finalNpmVerificationStep.indexOf("crate_root=$(mktemp -d)"),
);
for (const packageName of platformPackages) {
  assert.ok(finalNpmPlatformLoop.includes(packageName), `final npm verification must probe ${packageName}`);
}
const npmPreparationStep = release.slice(
  release.indexOf("- name: Verify release bundle and prepare npm packages"),
  release.indexOf("- name: Publish and verify each platform package in order"),
);
assert.match(
  npmPreparationStep,
  /cp LICENSE-MIT LICENSE-APACHE "npm\/\$pkg\/"/,
  "platform package staging must copy both license files",
);
assert.doesNotMatch(
  npmPreparationStep,
  /cp\s+README\.md\b[^\n]*npm\/\$pkg\//,
  "platform package staging must preserve each tracked package README",
);
const crateStateStep = release.slice(release.indexOf("id: crate-state"), release.indexOf("name: Publish crate when exact version is missing"));
const cratePublishStep = release.slice(release.indexOf("name: Publish crate when exact version is missing"), release.indexOf("- name: Verify published crate identity without publish credentials"));
const crateVerifyStep = release.slice(release.indexOf("- name: Verify published crate identity without publish credentials"), release.indexOf("  publish-npm:"));
assert.doesNotMatch(crateStateStep, /CARGO_REGISTRY_TOKEN/, "crate probes must not receive publication credentials");
assert.match(cratePublishStep, /CARGO_REGISTRY_TOKEN:[\s\S]*?cargo publish --locked/, "crate token must scope only publication");
assert.match(cratePublishStep, /unset CARGO_REGISTRY_TOKEN[\s\S]*?CARGO_REGISTRY_TOKEN=\"\$publish_token\" cargo publish --locked/, "crate token must be process-scoped");
assert.doesNotMatch(crateVerifyStep, /CARGO_REGISTRY_TOKEN/, "crate verification must not inherit publish credentials");
for (const job of ["version-check", "package", "publication-preflight", "github-release", "publish-crates", "publish-npm", "verify-channels"]) {
  assert.match(release, new RegExp(`${job}:[\\s\\S]*?runs-on: ubuntu-24\\.04`), `${job} should use the current x64 Linux runner`);
}
assert.equal((ci.match(/actions\/checkout@/g) ?? []).length, (ci.match(/persist-credentials: false/g) ?? []).length, "CI checkouts must not persist GitHub credentials");
assert.equal((release.match(/actions\/checkout@/g) ?? []).length, (release.match(/persist-credentials: false/g) ?? []).length, "release checkouts must not persist GitHub credentials");
