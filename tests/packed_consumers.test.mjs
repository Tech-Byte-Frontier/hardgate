// Exercise packed wrapper and optional-native archives through real npm and
// pnpm installs. No Cargo build or remote registry is involved.
"use strict";

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { aggregateCleanupErrors, checkPackedConsumers } from "../scripts/check-packed-consumers.mjs";
import { inspectPackedArtifacts, snapshotArchiveFiles, verifyArchiveSnapshot } from "../scripts/packed-consumer-artifacts.mjs";
import { MAX_ARCHIVE_BYTES } from "../scripts/packed-consumer-tar.mjs";
import { startLocalRegistry } from "../scripts/packed-consumer-registry.mjs";
import { resolveInstalledPackage, resolveWrapperBinary, verifyResolvedNative } from "../scripts/packed-consumer-runtime.mjs";
import {
  hash, httpStatus, makeDuplicateManifestArchives, makeFixtureArchives, makeSkippedOptionalRoot,
  makeUnsafeArchive, packModified,
} from "./packed_consumer_fixtures.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const fixtureRoot = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-packed-consumer-test-"));

try {
  const fixture = makeFixtureArchives({ fixtureRoot, root });
  const archiveFiles = fs.readdirSync(fixture.packagesDir).filter((name) => name.endsWith(".tgz"));
  const archiveHashes = new Map(archiveFiles.map((name) => [name, hash(path.join(fixture.packagesDir, name))]));
  const previousOverride = process.env.HARDGATE_BINARY;
  process.env.HARDGATE_BINARY = path.join(fixtureRoot, "does-not-exist");
  let report;
  try {
    report = await checkPackedConsumers({
      packagesDir: fixture.packagesDir,
      binary: fixture.nativeBinary,
      version: "0.6.0",
    });
  } finally {
    if (previousOverride === undefined) delete process.env.HARDGATE_BINARY;
    else process.env.HARDGATE_BINARY = previousOverride;
  }
  assert.equal(report.hostPackage, "hardgate-linux-x64");
  assert.equal(report.consumers.map((consumer) => `${consumer.manager}:${consumer.scope}`).join(","), "npm:project,pnpm:project,npm:global,pnpm:global");
  assert.equal(report.consumers.every((consumer) => consumer.nativeSha256 === report.binarySha256), true);
  assert.equal(report.consumers.every((consumer) => consumer.acceptance.passed && consumer.acceptance.testFailurePropagated && consumer.acceptance.inputsPreserved), true);
  assert.match(report.expectedVersionOutput, /^hardgate 0\.6\.0 \([0-9a-f]+\)$/);
  for (const [name, expected] of archiveHashes) assert.equal(hash(path.join(fixture.packagesDir, name)), expected, `${name} was rewritten`);

  const missingHost = path.join(fixtureRoot, "missing-host");
  fs.mkdirSync(missingHost);
  for (const name of archiveFiles) {
    if (!name.startsWith("hardgate-linux-x64-0.6.0")) fs.copyFileSync(path.join(fixture.packagesDir, name), path.join(missingHost, name));
  }
  await assert.rejects(
    checkPackedConsumers({ packagesDir: missingHost, binary: fixture.nativeBinary, version: "0.6.0" }),
    /missing host optional dependency hardgate-linux-x64/,
  );

  const badUrl = packModified({ fixtureRoot }, {
    sourceName: "hardgate-linux-x64", label: "url",
    mutate: (manifest) => {
      manifest.optionalDependencies = { "fixture-redirect": "https://registry.example.invalid/redirect.tgz" };
    },
  });
  await assert.rejects(
    checkPackedConsumers({ packagesDir: badUrl, binary: fixture.nativeBinary, version: "0.6.0" }),
    /optionalDependency fixture-redirect must be a registry version/,
  );

  const badPlatformDeps = packModified({ fixtureRoot }, {
    sourceName: "hardgate-linux-x64", label: "platform-deps",
    mutate: (manifest) => {
      manifest.optionalDependencies = { "fixture-extra": "0.6.0" };
    },
  });
  await assert.rejects(
    checkPackedConsumers({ packagesDir: badPlatformDeps, binary: fixture.nativeBinary, version: "0.6.0" }),
    /hardgate-linux-x64 must not declare optionalDependencies/,
  );

  const badHook = packModified({ fixtureRoot }, {
    sourceName: "hardgate", label: "hook",
    mutate: (manifest) => {
      manifest.scripts.postinstall = "curl https://registry.example.invalid/install";
    },
  });
  await assert.rejects(
    checkPackedConsumers({ packagesDir: badHook, binary: fixture.nativeBinary, version: "0.6.0" }),
    /must not declare npm lifecycle hook postinstall/,
  );

  const badDescriptor = packModified({ fixtureRoot }, {
    sourceName: "hardgate-linux-x64", label: "descriptor",
    mutate: (manifest) => {
      manifest.cpu = ["arm64"];
    },
  });
  await assert.rejects(
    checkPackedConsumers({ packagesDir: badDescriptor, binary: fixture.nativeBinary, version: "0.6.0" }),
    /hardgate-linux-x64 manifest cpu=\["arm64"\] expected \["x64"\]/,
  );

  const missingNative = packModified({ fixtureRoot }, {
    sourceName: "hardgate-linux-x64", label: "missing-native", mutate: () => {},
    mutateFiles: (packageDirectory) => {
      fs.rmSync(path.join(packageDirectory, "bin", "hardgate"));
    },
  });
  await assert.rejects(
    checkPackedConsumers({ packagesDir: missingNative, binary: fixture.nativeBinary, version: "0.6.0" }),
    /hardgate-linux-x64@0\.6\.0\.tgz is missing package\/bin\/hardgate/,
  );

  const badBin = packModified({ fixtureRoot }, {
    sourceName: "hardgate", label: "bin",
    mutate: (manifest) => {
      manifest.bin.hardgate = "bin/other.js";
    },
  });
  await assert.rejects(
    checkPackedConsumers({ packagesDir: badBin, binary: fixture.nativeBinary, version: "0.6.0" }),
    /manifest bin\.hardgate must be exactly bin\/hardgate\.js/,
  );

  const duplicate = makeDuplicateManifestArchives({ fixtureRoot });
  await assert.rejects(
    checkPackedConsumers({ packagesDir: duplicate.packagesDir, binary: fixture.nativeBinary, version: "0.6.0" }),
    /duplicate member: package\/package\.json/,
  );
  assert.equal(fs.existsSync(duplicate.marker), false, "duplicate manifest hook must not execute");

  assert.throws(
    () => inspectPackedArtifacts(makeUnsafeArchive({ fixtureRoot }), "0.6.0", fixture.nativeBinary),
    /member path is not canonical/,
  );

  const inspected = inspectPackedArtifacts(fixture.packagesDir, "0.6.0", fixture.nativeBinary);
  const registry = await startLocalRegistry(inspected.artifacts);
  try {
    const skipped = await makeSkippedOptionalRoot({ fixtureRoot, nativeBinary: fixture.nativeBinary, registry });
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
  fs.writeFileSync(path.join(ancestorRoot, "node_modules", "hardgate-linux-x64", "package.json"), JSON.stringify({ name: "hardgate-linux-x64", version: "0.6.0" }));
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
    () => inspectPackedArtifacts(oversizedDir, "0.6.0", fixture.nativeBinary),
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
