import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import fs from "node:fs";
import { PLATFORM_NAMES } from "../scripts/release-platforms.mjs";
import { mirrorVersion, packageName, verifyArchive } from "../scripts/mirror-github-package.mjs";

const source = Buffer.from("immutable wrapper archive");
const version = "0.6.1";
const manifest = {
  name: packageName,
  version,
  repository: { url: "git+https://github.com/Tech-Byte-Frontier/hardgate.git" },
  optionalDependencies: Object.fromEntries(PLATFORM_NAMES.map(name => [name, version])),
  dist: { integrity: `sha512-${createHash("sha512").update(source).digest("base64")}` },
};
verifyArchive(source, manifest, version, manifest);
assert.throws(() => verifyArchive(Buffer.from("tampered"), manifest, version, manifest), /integrity/);
for (const invalid of [
  { name: "hardgate" }, { version: "0.6.2" }, { repository: { url: "https://example.com" } },
  { optionalDependencies: { "hardgate-linux-x64": "0.6.1" } },
]) assert.throws(() => verifyArchive(source, { ...manifest, ...invalid }, version, manifest));

// Historical releases use their own signed manifest, never today's platform set.
const historical = { ...manifest, version: "0.6.0", optionalDependencies: { "hardgate-linux-x64": "0.6.0" } };
verifyArchive(source, historical, "0.6.0", historical);
assert.throws(() => verifyArchive(source, manifest, version, historical), /signed package version/);

let writes = 0;
let verifications = 0;
const operations = {
  source,
  probe: async () => manifest,
  publish: async () => { writes += 1; },
  verify: async (observed, expected) => {
    assert.equal(observed, manifest);
    assert.equal(expected, source);
    verifications += 1;
  },
};
assert.equal(await mirrorVersion(operations), "reused");
assert.equal(writes, 0);
assert.equal(verifications, 1);

let probes = 0;
assert.equal(await mirrorVersion({ ...operations, probe: async () => ++probes === 1 ? null : manifest }), "published");
assert.equal(writes, 1);
assert.equal(verifications, 2);

await assert.rejects(mirrorVersion({ ...operations, probe: async () => { throw new Error("HTTP 403"); } }), /403/);
await assert.rejects(mirrorVersion({ ...operations, verify: async () => { throw new Error("mismatched bytes"); } }), /mismatched/);
assert.equal(writes, 1, "auth errors and conflicting existing packages must never trigger publication");

probes = 0;
await assert.rejects(mirrorVersion({
  ...operations,
  probe: async () => { probes += 1; return null; },
  publish: async () => { writes += 1; throw new Error("ambiguous publish timeout"); },
}), /ambiguous/);
assert.equal(writes, 2, "ambiguous publication must not be retried");
assert.equal(probes, 1);

const workflow = fs.readFileSync(new URL("../.github/workflows/github-packages.yml", import.meta.url), "utf8");
for (const line of workflow.split("\n").filter((value) => value.includes("uses:"))) {
  assert.match(line, /@[0-9a-f]{40}\b/);
}
for (const required of [
  "workflows: [Release]", "github.event.workflow_run.conclusion == 'success'",
  "group: hardgate-release", "packages: write", "persist-credentials: false",
  "verify-tag", "git merge-base --is-ancestor", 'test "$REQUESTED_TAG" = "$tag"',
  "scope: '@tech-byte-frontier'", "--registry=https://registry.npmjs.org",
  'hardgate $RELEASE_VERSION ($RELEASE_COMMIT)',
]) assert.ok(workflow.includes(required), `missing mirror boundary: ${required}`);
assert.doesNotMatch(workflow, /secrets\.NPM_TOKEN|id-token: write|continue-on-error|pull_request/);
console.log("github_package_mirror.test: OK");
