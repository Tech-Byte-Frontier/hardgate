// Registry consumer proof for native local-analysis platforms. The Linux x64
// consumer separately verifies complete isolated execution and the wrapper receipt.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { spawnSync } from "node:child_process";
import { npm, run, verifyLocalAnalysis } from "./local-consumer.mjs";
import { executableName } from "./release-platforms.mjs";
import { detectHost, hostNativePackage, digestFile, digestBytes, stableExecutablePath, parseArgs } from "./native-channel-support.mjs";
import { validateProof } from "./native-channel-proof.mjs";

const values = parseArgs(process.argv.slice(2));
const { packageName, version, sourceSha, archive, mode, output } = values;
assert.equal(packageName, hostNativePackage(detectHost()), "consumer must run on its native host");
assert.notEqual(packageName, "hardgate-linux-x64", "use the complete Linux consumer for this host");
assert.ok(values.wrapperSource, "--wrapper-source is required to compare the installed launcher");
const checksum = fs.readFileSync(path.join(path.dirname(archive), "SHA256SUMS"), "utf8")
  .split(/\r?\n/).find((line) => line.endsWith(`  ${packageName}.tar.gz`))?.split("  ")[0];
assert.equal(digestFile(archive), checksum, "release archive checksum");
const member = `${packageName}/${executableName(packageName)}`;
const extracted = spawnSync("tar", ["-xOzf", archive, member], { encoding: null, timeout: 30_000, maxBuffer: 128 * 1024 * 1024 });
assert.ifError(extracted.error);
assert.equal(extracted.status, 0, "release archive must contain the native executable");
const expected = extracted.stdout;
assert.ok(expected.includes(Buffer.from(`${version} (${sourceSha})`)), "release binary identity");

const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-registry-consumer-"));
try {
  const env = {};
  for (const key of ["PATH", "Path", "SystemRoot", "WINDIR", "ComSpec", "PATHEXT", "LANG", "LC_ALL"]) {
    if (process.env[key] !== undefined) env[key] = process.env[key];
  }
  Object.assign(env, {
    HOME: temporary, USERPROFILE: temporary, TMP: temporary, TEMP: temporary, TMPDIR: temporary,
    npm_config_cache: path.join(temporary, "cache"),
    npm_config_userconfig: path.join(temporary, "npmrc"),
    npm_config_globalconfig: path.join(temporary, "global-npmrc"),
  });
  fs.writeFileSync(path.join(temporary, "package.json"), '{"private":true}\n');
  const selected = mode === "exact" ? version : "latest";
  npm(["install", "--ignore-scripts", "--no-audit", "--no-fund", "--registry=https://registry.npmjs.org/",
    `${packageName}@${selected}`, `@tech-byte-frontier/hardgate@${selected}`], { cwd: temporary, env });
  const packageRoot = path.join(temporary, "node_modules", packageName);
  const wrapperRoot = path.join(temporary, "node_modules/@tech-byte-frontier/hardgate");
  for (const [directory, name] of [[packageRoot, packageName], [wrapperRoot, "@tech-byte-frontier/hardgate"]]) {
    const manifest = JSON.parse(fs.readFileSync(path.join(directory, "package.json"), "utf8"));
    assert.equal(manifest.name, name);
    assert.equal(manifest.version, version, "registry selection must resolve to the release version");
  }
  const binary = path.join(packageRoot, "bin", executableName(packageName));
  assert.deepEqual(fs.readFileSync(binary), expected, "installed native bytes must match the verified archive");
  const launcher = path.join(wrapperRoot, "bin/hardgate.js");
  assert.deepEqual(fs.readFileSync(launcher), fs.readFileSync(values.wrapperSource));
  const require = createRequire(import.meta.url);
  assert.equal(fs.realpathSync(require(launcher).findBinary()), fs.realpathSync(binary));
  assert.equal(run(process.execPath, [launcher, "--version"], { env }).trim(), `hardgate ${version} (${sourceSha})`);
  verifyLocalAnalysis(binary, [], env);
  verifyLocalAnalysis(process.execPath, [launcher], env);
  const proof = validateProof({
    schema_version: 1, version, source_sha: sourceSha, mode, package: packageName,
    archive: { name: `${packageName}.tar.gz`, sha256: checksum },
    consumer: { executable: stableExecutablePath(packageName), sha256: digestBytes(expected) },
  });
  fs.writeFileSync(output, `${JSON.stringify(proof, null, 2)}\n`, { flag: "wx" });
  console.log(`portable registry consumer: ${packageName}@${selected} verified`);
} finally {
  fs.rmSync(temporary, { recursive: true, force: true });
}
