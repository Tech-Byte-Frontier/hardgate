#!/usr/bin/env node
// Mirror the immutable npm wrapper; the native dependency stays on npmjs.org.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { setTimeout as delay } from "node:timers/promises";
import { runReleaseProcess } from "./release-process.mjs";

export const packageName = "@tech-byte-frontier/hardgate";
const npmRegistry = "https://registry.npmjs.org";
const githubRegistry = "https://npm.pkg.github.com";
const repository = "git+https://github.com/Tech-Byte-Frontier/hardgate.git";

export function verifyArchive(bytes, manifest, version, expected) {
  assert.equal(expected.name, packageName, "unexpected signed package name");
  assert.equal(expected.version, version, "unexpected signed package version");
  assert.equal(manifest.name, packageName, "unexpected package name");
  assert.equal(manifest.version, version, "unexpected package version");
  assert.equal(manifest.repository?.url, repository, "unexpected repository");
  assert.deepEqual(manifest.optionalDependencies, expected.optionalDependencies);
  const integrity = `sha512-${createHash("sha512").update(bytes).digest("base64")}`;
  assert.equal(manifest.dist?.integrity, integrity, "archive integrity mismatch");
}

async function request(url, token, allowMissing = false) {
  const response = await fetch(url, {
    headers: token ? { authorization: `Bearer ${token}` } : {},
    signal: AbortSignal.timeout(20_000),
  });
  if (allowMissing && response.status === 404) return null;
  if (!response.ok) throw new Error(`Registry request failed: HTTP ${response.status}`);
  const chunks = [];
  let size = 0;
  for await (const chunk of response.body) {
    size += chunk.length;
    if (size > 4 * 1024 * 1024) throw new Error("Wrapper registry response exceeds 4 MiB");
    chunks.push(chunk);
  }
  return Buffer.concat(chunks);
}

async function metadata(registry, token) {
  const bytes = await request(`${registry}/${encodeURIComponent(packageName)}`, token, true);
  return bytes === null ? null : JSON.parse(bytes.toString("utf8"));
}

async function archive(manifest, registry, token) {
  const url = new URL(manifest.dist.tarball);
  assert.equal(url.origin, registry, "unexpected tarball origin");
  assert.equal(url.username + url.password, "", "unexpected tarball credentials");
  return request(url, token);
}

// A rerun reuses identical bytes. Publication is attempted only once per run;
// ambiguous failures stop and require registry inspection before a rerun.
export async function mirrorVersion({ source, probe, publish, verify }) {
  const existing = await probe();
  if (existing) {
    await verify(existing, source);
    return "reused";
  }
  await publish();
  for (let attempt = 0; attempt < 10; attempt += 1) {
    const observed = await probe();
    if (observed) {
      await verify(observed, source);
      return "published";
    }
    if (attempt < 9) await delay(2000);
  }
  throw new Error("Published version is not visible; inspect GitHub Packages before rerunning");
}

async function main() {
  const [version, manifestPath] = process.argv.slice(2);
  assert.match(version ?? "", /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/);
  const token = process.env.NODE_AUTH_TOKEN;
  assert.ok(token, "NODE_AUTH_TOKEN is required");
  const expected = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  const sourceMetadata = await metadata(npmRegistry);
  assert.equal(sourceMetadata?.["dist-tags"]?.latest, version, "mirror only npm latest");
  const manifest = sourceMetadata.versions[version];
  const source = await archive(manifest, npmRegistry);
  verifyArchive(source, manifest, version, expected);
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-github-mirror-"));
  const run = (args) => runReleaseProcess("npm", args, { cwd: directory, timeoutMs: 120_000 });
  try {
    const filename = path.join(directory, "wrapper.tgz");
    fs.writeFileSync(filename, source);
    const result = await mirrorVersion({
      source,
      probe: async () => (await metadata(githubRegistry, token))?.versions?.[version],
      publish: () => run([
        "publish", filename, `--registry=${githubRegistry}`, "--provenance=false",
        "--ignore-scripts", "--tag=latest", "--access=public", "--fetch-retries=0",
      ]),
      verify: async (observed, expected) => {
        const bytes = await archive(observed, githubRegistry, token);
        verifyArchive(bytes, observed, version, expected);
        assert.ok(bytes.equals(expected), "GitHub mirror differs from npm archive");
      },
    });
    // An existing immutable version may need its default restored after a
    // previous partial run. Recheck the source default before moving ours.
    assert.equal((await metadata(npmRegistry))?.["dist-tags"]?.latest, version);
    if ((await metadata(githubRegistry, token))?.["dist-tags"]?.latest !== version) {
      await run(["dist-tag", "add", `${packageName}@${version}`, "latest", `--registry=${githubRegistry}`, "--fetch-retries=0"]);
    }
    assert.equal((await metadata(githubRegistry, token))?.["dist-tags"]?.latest, version);
    console.log(`GitHub Packages: ${result} ${packageName}@${version}; archive bytes verified`);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main().catch((error) => { console.error(error.message); process.exitCode = 1; });
}
