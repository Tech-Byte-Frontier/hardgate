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
  nodeVersion,
  npmPackRetry,
  npmPlatformDirectories,
  npmPublication,
  npmRegistryPack,
  npmVerificationPolicy,
  packageScript,
  platformPackages,
  release,
  releaseJob,
  directConsumer,
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
    "/SECURITY.md",
    "/CHANGELOG.md",
    "/LICENSE-MIT",
    "/LICENSE-APACHE",
    "/docs/INSTALLATION.md",
    "/docs/GETTING_STARTED.md",
    "/docs/TRIALS_0_6.md",
    "/docs/MUTATION_RESOURCES.md",
    "/docs/ARCHITECTURE.md",
    "/docs/CLI_AND_INTEGRATION.md",
    "/docs/REPORT_SCHEMA.md",
    "/docs/DIAGNOSTIC_RULES.md",
    "/docs/CONFIGURATION_SPEC.md",
    "/docs/EXISTING_LANDSCAPE.md",
    "/docs/VISION_AND_PARADIGM.md",
  ],
  "Cargo package allowlist must exclude workspace, generated, release, and private local artifacts",
);
includesAll(verifier, ["MAX_BINARY_BYTES", "verifyEmbeddedIdentity", "verifyExecutableMember", "tar", "-tvzf", "fs.chmodSync(binaryPath, 0o755)", "Buffer.from(`${version} (${commit})`", "hardgate-target:", "expected Cargo target marker", "expectedOutput", "result.stdout.trim() !== expectedOutput", "verifyBinaryAbi", "readelf", "-l", "-sW", "-n", "classifyBinaryAbi"], "archive verifier");
includesAll(releaseAbi, ["classifyBinaryAbi", "no positive ELF/glibc ABI evidence", "ld-musl", "GLIBC_", "2.39 baseline", 'abi !== "gnu"'], "GNU ABI evidence classifier");
includesAll(npmPublication, ["--platform-only", "--package", "optionalDependencies", "byte-match", "path.join(packageDirectory, \"bin/hardgate\")", "tar", "-tvzf", "npm/hardgate/bin/hardgate.js"], "npm publication verifier");
includesAll(npmRegistryPack, ["npm pack", "--loglevel=error", "isRetryableNpmPackError", "failed without retry", "exactVersionObserved", "childTimeoutMs"], "npm registry retrieval");
includesAll(npmVerificationPolicy, ["NPM_VERIFY_ATTEMPTS", "NPM_VERIFY_TIMEOUT_SECONDS", "NPM_VERIFY_CHILD_TIMEOUT_SECONDS", "remainingMs"], "npm verification deadlines");
assert.doesNotMatch(npmRegistryPack.slice(npmRegistryPack.indexOf("async function packOnce"), npmRegistryPack.indexOf("async function mayRetry")), /["']--silent["']/, "npm pack must retain diagnostics needed to classify transient registry failures");
includesAll(npmPackRetry, ["isRetryableNpmPackError", "E404", "EAI_AGAIN", "ECONNRESET", "ETIMEDOUT", "ECONNREFUSED"], "npm pack retry classifier");
for (const error of [
  { code: "E404" },
  { message: "npm ERR! HTTP 404" },
  { stderr: "npm error code E404\nnpm error 404 Not Found" },
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
includesAll(sbomScript, ["$schema", "serialNumber", "uuidV5(JSON.stringify(bom))", "rootComponent.type = \"application\"", "components.filter", "codepointCompare", "Buffer.from(left, \"utf8\").compare"], "CycloneDX structure");
assert.doesNotMatch(sbomScript, /id:\s*pkg\.license/, "raw package SPDX expressions must not be emitted as license ids");
includesAll(sbomVerifier, ["CycloneDX", "1.5", "serialNumber", "RFC 4122 UUIDv5 URN", "uuidV5(JSON.stringify(withoutSerial))", "metadata.component", "application", "must not be duplicated", "license.expression", "$schema"], "CycloneDX verifier");
includesAll(coverageScript, ["CARGO_LLVM_COV_VERSION", "COV_TOOLCHAIN=\"${RUST_COVERAGE_TOOLCHAIN:-nightly-2026-09-04}\"", "0.9.0", "HARDGATE_REQUIRE_PREINSTALLED_CARGO_TOOLS", "expected preinstalled cargo-llvm-cov", "cargo install cargo-llvm-cov --version \"=$COV_VERSION\"", "cargo \"+$COV_TOOLCHAIN\" llvm-cov --version", "evidence cargo-llvm-cov", "--toolchain", "--all-features", ".hardgate/evidence/coverage.lcov", "coverage.lcov.hardgate.json"], "coverage helper");
includesAll(auditScript, ["CARGO_AUDIT_VERSION", "0.22.2", "HARDGATE_REQUIRE_PREINSTALLED_CARGO_TOOLS", "expected preinstalled cargo-audit", "cargo install cargo-audit --version \"=$AUDIT_VERSION\"", "cargo audit"], "audit helper");
includesAll(selfGate, ["check --format agent", "check --checks policy", "--coverage-report .hardgate/evidence/coverage.lcov --format agent", "evidence cargo-mutants", "--file src/engines/budgets.rs", "replace check_measured_budgets", "--mutation-report \"$HARDGATE_MUTATION_REPORT\"", "HARDGATE_BINARY=\"$BINARY\" node scripts/check-consumer-matrix.mjs"], "self gate");
assert.doesNotMatch(selfGate, /TEMP_POLICY|enabled = false/, "self gate must require source-bound evidence without rewriting its policy");

for (const target of targets) assert.ok(release.includes(target), `release must build ${target}`);
for (const packageName of platformPackages) {
  assert.ok(release.includes(packageName), `release must handle ${packageName}`);
}
assert.deepEqual(npmPlatformDirectories, [...platformPackages].sort(), "npm directories must match the supported platform set");
assert.deepEqual(Object.keys(wrapperManifest.optionalDependencies ?? {}).sort(), [...platformPackages].sort(), "wrapper optionalDependencies must match the supported platform set");
assert.doesNotMatch(release, /matrix:|hardgate-linux-x64-musl|hardgate-linux-arm64|hardgate-darwin/);
assert.match(release, /native-linux-x64-attempt-/, "release must reuse the exact successful CI artifact");
const packageJob = releaseJob("package");
const publishJob = releaseJob("publish");
assert.doesNotMatch(packageJob, /actions\/attest@|id-token: write|attestations: write/, "verified packaging must remain reusable if attestation fails");
assert.match(packageJob.trimEnd(), /retention-days: 30$/, "bundle upload must remain the final packaging checkpoint");
includesAll(packageJob, ["needs.version-check.outputs.ci_native_artifact_id", "needs.version-check.outputs.ci_run_id", "build-binaries/binary-x86_64-unknown-linux-gnu", "resume_artifact_id", "github-token: ${{ github.token }}", "run-id: ${{ inputs.resume_run_id }}", "digest-mismatch: error"], "CI and resume artifact identities");
includesAll(publishJob, ["subject-checksums", "sbom-path", "attestations: write", "artifact-metadata: write", "id-token: write"], "checksum and SBOM provenance");
assert.equal((publishJob.match(/actions\/attest@/g) ?? []).length, 2);
assert.ok(publishJob.indexOf("actions/attest@") < publishJob.indexOf("scripts/stage-github-release.mjs"));
assert.doesNotMatch(publishJob, /continue-on-error/);
includesAll(directConsumer, ['cmp -- "dist/$asset"', "sha256sum --check --strict", "installed-check.mjs", "--consumer"], "direct downloaded bytes and runtime acceptance");

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

const releaseCurlInvocations = release.match(/\bcurl\s+--/g) ?? [];
assert.ok(releaseCurlInvocations.length > 0, "release must probe registries with curl");
assert.equal((release.match(/--connect-timeout/g) ?? []).length, releaseCurlInvocations.length, "every release curl must set connect timeout");
assert.equal((release.match(/--max-time/g) ?? []).length, releaseCurlInvocations.length, "every release curl must set max time");
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
