// Static safety contracts around CI promotion and the six-stage release flow.
"use strict";
import assert from "node:assert/strict";
import { ci, includesAll, installedConsumers, launcher, release, releaseJob } from "./release_contract.sources.mjs";

for (const [label, text] of [["CI", ci], ["release", release]]) {
  for (const line of text.split("\n").filter((value) => value.includes("uses:"))) {
    assert.match(line, /@[0-9a-f]{40}\b/i, `${label} action must be immutable`);
    assert.match(line, /#\s*(?:v?[0-9]|master\b)/i, `${label} pin must identify its version`);
  }
  assert.equal((text.match(/actions\/checkout@/g) ?? []).length, (text.match(/persist-credentials: false/g) ?? []).length, `${label} must not persist checkout credentials`);
  assert.doesNotMatch(text, /YARN_VERSION|BUN_VERSION|setup-bun|macos-|ubuntu-24\.04-arm|matrix:/);
  includesAll(text, ["NODE_VERSION: 26.8.1", "NPM_VERSION: 12.0.2", "PNPM_VERSION: 11.25.0", "digest-mismatch: error", "retention-days: 30"], `${label} tool and artifact contracts`);
}
includesAll(ci, [
  "cargo fmt --all --check", "cargo clippy --all-targets --all-features --locked -- -D warnings",
  "cargo test --all-targets --all-features --locked", "scripts/dependency-audit.sh", "cargo publish --dry-run --locked",
  "scripts/self-gate.sh", "CARGO_AUDIT_VERSION: 0.22.2", "CARGO_LLVM_COV_VERSION: 0.9.0", "CARGO_MUTANTS_VERSION: 27.1.0",
  "RUST_COVERAGE_TOOLCHAIN: nightly-2026-09-04", 'HARDGATE_REQUIRE_PREINSTALLED_CARGO_TOOLS: "1"', "fallback: none",
  "components: rustfmt, clippy, llvm-tools-preview", "node scripts/check-npm-quality.mjs", "node tests/npm-wrapper.test.mjs",
  "node tests/npm-wrapper-regression.test.mjs", "node tests/consumer_matrix.mjs", "node tests/release_contract.sbom.test.mjs",
  "node tests/packed_consumers.test.mjs", "name: CI quality aggregate", "contains(needs.*.result, 'skipped')",
  "name: Select distribution checks", "distribution=true", "EVENT_NAME", "BASE_SHA", "HEAD_SHA",
  "if: needs.rust.outputs.distribution == 'true'",
], "focused required CI");
const ciRust = ci.slice(ci.indexOf("  rust:"), ci.indexOf("  npm-wrapper:"));
const ciSelf = ci.slice(ci.indexOf("  hardgate-self:"), ci.indexOf("  release-contract:"));
const ciWrapper = ci.slice(ci.indexOf("  npm-wrapper:"), ci.indexOf("  npm-wrapper-minimum:"));
assert.equal((ci.match(/cargo build --locked --release/g) ?? []).length, 1, "CI builds the native release binary once");
assert.doesNotMatch(release, /cargo build/, "release reuses the exact CI artifact");
includesAll(ciRust, ["steps.upload_native.outputs.artifact-id", "SOURCE_DATE_EPOCH: 0", "native-linux-x64-attempt-", "cargo publish --dry-run --locked"], "source-identified CI artifact");
assert.doesNotMatch(ciRust, /CARGO_REGISTRY_TOKEN|NODE_AUTH_TOKEN/, "CI build scripts must not receive publication credentials");
for (const section of [ciSelf, ciWrapper]) {
  includesAll(section, ["needs: rust", "artifact-ids: ${{ needs.rust.outputs.native_artifact_id }}", "chmod 755 target/release/hardgate"], "reused CI binary");
  assert.doesNotMatch(section, /cargo build --locked --release/);
}
assert.ok(ciSelf.indexOf("toolchain: ${{ env.RUST_COVERAGE_TOOLCHAIN }}") < ciSelf.indexOf("toolchain: ${{ env.RUST_TOOLCHAIN }}"), "stable Rust remains the default after installing coverage nightly");
for (const tool of ["cargo-llvm-cov", "cargo-mutants"]) assert.ok(ciSelf.includes(`tool: ${tool}@`));
for (const command of ["cargo fmt --all --check", "cargo clippy --all-targets --all-features --locked -- -D warnings", "cargo test --all-targets --all-features --locked", "scripts/dependency-audit.sh", "scripts/self-gate.sh"]) {
  assert.ok(!release.includes(command), `release must not repeat full CI gate ${command}`);
}
assert.doesNotMatch(release, /--clobber|overwrite:\s*true|continue-on-error/, "immutable assets and checkpoint failures must never be bypassed");
assert.doesNotMatch(release, /npm view|https:\/\/crates\.io\/api\/v1\/me/, "publication must use status-aware anonymous probes");
includesAll(release, ["retry_absent", "return 3", "--user-agent \"$HARDGATE_CRATES_IO_USER_AGENT\"", "HARDGATE_NPM_VISIBILITY_TIMEOUT_SECONDS: 580"], "bounded registry classification");
const registryAttempts = Number(release.match(/HARDGATE_REGISTRY_ATTEMPTS:\s*(\d+)/)?.[1]);
const registryDelay = Number(release.match(/HARDGATE_REGISTRY_DELAY:\s*(\d+)/)?.[1]);
const curlMaxTime = Number(release.match(/HARDGATE_CURL_MAX_TIME:\s*(\d+)/)?.[1]);
assert.ok(registryAttempts * curlMaxTime + (registryAttempts - 1) * registryDelay <= 300);
assert.ok(580 + curlMaxTime <= 600);
const publish = releaseJob("publish");
assert.match(publish, /test "\$\(git -C release-tooling rev-parse HEAD\)" = "\$GITHUB_SHA"[\s\S]*?node release-tooling\/scripts\/stage-github-release\.mjs/);
assert.doesNotMatch(publish, /gh release (?:create|upload)/, "writes use the tested staging state machine");
assert.ok(publish.indexOf("Publish and verify each platform package") < publish.indexOf("Publish wrapper only after all platforms are verified"));
const crateState = publish.slice(publish.indexOf("id: crate-state"), publish.indexOf("name: Publish crate when exact version is missing"));
const crateWrite = publish.slice(publish.indexOf("name: Publish crate when exact version is missing"), publish.indexOf("- name: Verify published crate identity without publish credentials"));
const crateVerify = publish.slice(publish.indexOf("- name: Verify published crate identity without publish credentials"), publish.indexOf("- name: Select @tech-byte-frontier/hardgate"));
assert.doesNotMatch(crateState, /CARGO_REGISTRY_TOKEN/);
assert.match(crateWrite, /unset CARGO_REGISTRY_TOKEN[\s\S]*?CARGO_REGISTRY_TOKEN="\$publish_token" scripts\/with-resource-limits\.sh cargo publish --locked --no-verify/);
assert.doesNotMatch(crateVerify, /CARGO_REGISTRY_TOKEN/);
includesAll(crateVerify, ["verify-crate-publication.mjs", 'cargo install hardgate --version "=$RELEASE_VERSION"', 'installed-check.mjs "$crate_root/bin/hardgate"', "--to exact_consumer_verified"], "real exact crate consumer");
const preparation = publish.slice(publish.indexOf("name: Verify release bundle and prepare npm packages"), publish.indexOf("name: Publish and verify each platform package"));
assert.match(preparation, /cp LICENSE-MIT LICENSE-APACHE "npm\/\$pkg\/"/);
assert.doesNotMatch(preparation, /cp\s+README\.md\b[^\n]*npm\/\$pkg\//, "platform README is preserved");
for (const jobName of ["package", "publish", "verify-exact", "promote-channels", "verify-channels"]) {
  const job = releaseJob(jobName);
  assert.match(job, /ref: \$\{\{ github\.sha \}\}[\s\S]{0,120}path: release-tooling/);
  assert.match(job, /runs-on: ubuntu-24\.04/);
}
for (const jobName of ["publish", "verify-channels"]) assert.ok(releaseJob(jobName).includes("cmp -- npm/hardgate/bin/hardgate.js release-tooling/npm/hardgate/bin/hardgate.js"));
assert.doesNotMatch(release, /node scripts\/verify-npm-publication\.mjs/, "payload scripts cannot shadow reviewed recovery fixes");
const consumers = releaseJob("verify-channels");
includesAll(consumers, ["--json tagName,isDraft,isPrerelease", 'test "$release_tag" = "$RELEASE_TAG"', 'test "$release_is_draft" = false', 'test "$release_is_prerelease" = false', 'test "$latest_release_tag" = "$RELEASE_TAG"', 'cmp -- "dist/$asset" "published-dist/$asset"'], "verified current default release");
includesAll(installedConsumers, ['"$npm_tool" install --ignore-scripts --global', '"$pnpm_tool" add --ignore-scripts --global', '"$pnpm_tool" bin --global', "command -v hardgate", "installed-check.mjs"], "installed project/global consumers");
includesAll(launcher, ["function detectMusl", "glibcVersionRuntime", "trim().length", "function exitFromSpawn", "result.status ?? 1", "process.kill(process.pid, result.signal)", "process.exit(1)"], "launcher libc and process contract");
assert.doesNotMatch(launcher, /fallbackPackages|hasAlpineRelease|MACHO_U32/);
