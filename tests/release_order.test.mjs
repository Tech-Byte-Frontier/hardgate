// SemVer precedence contract for protecting public latest channels.
"use strict";

import assert from "node:assert/strict";
import {
  assertReleaseDoesNotRegress,
  compareReleaseTags,
} from "../scripts/release-order.mjs";
import { includesAll, release } from "./release_contract.sources.mjs";

const publicationPreflight = release.slice(release.indexOf("  publication-preflight:"), release.indexOf("  github-release:"));
includesAll(
  publicationPreflight,
  [
    "Prevent latest-channel rollback",
    'repos/${GITHUB_REPOSITORY}/releases/latest',
    'https://registry.npmjs.org/${encoded_name}/latest',
    'node scripts/release-order.mjs --target-tag "$RELEASE_TAG" --latest-tag "$latest_tag"',
    'node scripts/release-order.mjs --target-tag "v$RELEASE_VERSION" --latest-tag "v$latest_version"',
    "unable to determine the current GitHub latest release; refusing registry publication",
    "npm latest probe for $package_name returned HTTP $npm_status; refusing registry publication",
  ],
  "pre-publication latest-channel rollback guard",
);
for (const packageName of [
  "hardgate-linux-x64",
  "hardgate-linux-x64-musl",
  "hardgate-linux-arm64",
  "hardgate-linux-arm64-musl",
  "hardgate-darwin-x64",
  "hardgate-darwin-arm64",
  "@tech-byte-frontier/hardgate",
]) {
  assert.ok(publicationPreflight.includes(packageName), `rollback guard must inspect npm latest for ${packageName}`);
}
assert.ok(
  publicationPreflight.indexOf("Prevent latest-channel rollback") < publicationPreflight.indexOf("Authenticate the npm publication credential"),
  "release ordering must be proven before publication credentials are used",
);

for (const [target, latest] of [
  ["v0.5.0", "v0.5.0"],
  ["v0.5.1", "v0.5.0"],
  ["v0.6.0", "v0.5.99"],
  ["v1.0.0", "v0.999.999"],
  ["v1.0.0", "v1.0.0-rc.1"],
  ["v1.0.0-rc.2", "v1.0.0-rc.1"],
  ["v1.0.0-beta.11", "v1.0.0-beta.2"],
]) {
  assert.doesNotThrow(() => assertReleaseDoesNotRegress(target, latest), `${target} should not regress ${latest}`);
}

for (const [target, latest] of [
  ["v0.4.9", "v0.5.0"],
  ["v0.5.0-rc.1", "v0.5.0"],
  ["v1.0.0-beta.2", "v1.0.0-beta.11"],
  ["v1.0.0+second", "v1.0.0+first"],
]) {
  assert.throws(() => assertReleaseDoesNotRegress(target, latest), /not newer than or identical/, `${target} should regress ${latest}`);
}

assert.equal(compareReleaseTags("v1.0.0-alpha", "v1.0.0-1"), 1);
assert.equal(compareReleaseTags("v1.0.0-alpha.1", "v1.0.0-alpha"), 1);
assert.equal(compareReleaseTags("v1.0.0+build.2", "v1.0.0+build.1"), 0);
for (const invalid of ["1.0.0", "v01.0.0", "v1.0", "v1.0.0-01", "v1.0.0-"]) {
  assert.throws(() => compareReleaseTags(invalid, "v1.0.0"), /invalid|leading zeroes/);
}
