// Verify actual global npm/pnpm installations using only the supplied archives.
"use strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { boundedProcess, cleanConsumerEnvironment, invocationEnvironment, managerPath } from "./packed-consumer-runtime.mjs";
import { verifyInstalledCheck } from "./installed-check.mjs";

const verifier = fileURLToPath(new URL("./verify-installed-wrapper.mjs", import.meta.url));

export async function installAndVerifyGlobal({ manager, root, registry, version, expectedOutput, expectedHash, expectedBinary, wrapperLauncherBytes, tempRoot }) {
  const prefix = path.join(root, "global-prefix");
  const cache = path.join(root, "cache");
  const store = path.join(prefix, "store");
  const env = cleanConsumerEnvironment(root, registry.baseUrl, cache, store);
  env.PNPM_HOME = prefix;
  const bin = path.join(prefix, "bin");
  fs.mkdirSync(bin, { recursive: true });
  env.PATH = `${bin}:${env.PATH}`;
  const tool = managerPath(manager);
  const spec = `@tech-byte-frontier/hardgate@${version}`;
  const args = manager === "npm"
    ? ["install", "--ignore-scripts", "--global", "--prefix", prefix, "--registry", registry.baseUrl, "--include=optional", spec]
    : ["add", "--ignore-scripts", "--global", "--store-dir", store, "--registry", registry.baseUrl, spec];
  await boundedProcess(tool, args, { cwd: root, env }, `${manager} global install`);
  if (manager === "pnpm") {
    const actualBin = (await boundedProcess(tool, ["bin", "--global"], { cwd: root, env }, "pnpm global bin")).trim();
    if (actualBin !== bin) throw new Error(`pnpm global bin ${actualBin} differs from ${bin}`);
  }
  const command = path.join(bin, "hardgate");
  const runtime = invocationEnvironment(root, env);
  const selected = (await boundedProcess("sh", ["-c", "command -v hardgate"], { cwd: root, env: { ...runtime, PATH: `${bin}:${runtime.PATH}` } }, `${manager} command resolution`)).trim();
  if (selected !== command) throw new Error(`${manager} global command escaped its fresh prefix`);
  const launcher = path.join(root, "expected-launcher.js");
  fs.writeFileSync(launcher, wrapperLauncherBytes);
  await boundedProcess(process.execPath, [verifier, prefix, command, expectedBinary, launcher, version, `${manager}-global`], { cwd: root, env: runtime }, `${manager} global byte identity`);
  const output = (await boundedProcess(command, ["--version"], { cwd: root, env: runtime }, `${manager} global version`)).trim();
  if (output !== expectedOutput) throw new Error(`${manager} global version identity differs`);
  const acceptance = await verifyInstalledCheck(command, { parent: tempRoot, env: runtime });
  return { manager, scope: "global", nativeSha256: expectedHash, versionOutput: output, acceptance };
}
