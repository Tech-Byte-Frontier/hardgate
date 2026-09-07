// Install actual packed wrapper/native artifacts on each supported CI host.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { npm, run, verifyLocalAnalysis } from "../scripts/local-consumer.mjs";
import { executableName } from "../scripts/release-platforms.mjs";
import { verifyWindowsRuntime } from "../scripts/windows-runtime.mjs";

const require = createRequire(import.meta.url);
const launcher = require("../npm/hardgate/bin/hardgate.js");
const root = fileURLToPath(new URL("../", import.meta.url));
const host = launcher.platformPackage();
assert.ok(host);
const binary = path.resolve(process.argv[2] ?? path.join(root, "target/debug", executableName(host)));
if (process.platform === "win32") verifyWindowsRuntime(fs.readFileSync(binary));
verifyLocalAnalysis(binary);
const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "hardgate-native-install-"));
try {
  const env = { ...process.env, npm_config_cache: path.join(temporary, "cache") };
  for (const key of ["HARDGATE_BINARY", "HARDGATE_BINARY_PATH", "HARDGATE_LAUNCHER_DEPTH", "NODE_OPTIONS"]) delete env[key];
  const archives = [];
  for (const name of ["hardgate", host]) {
    const directory = path.join(temporary, name);
    fs.cpSync(path.join(root, "npm", name), directory, { recursive: true });
    if (name === host) {
      fs.mkdirSync(path.join(directory, "bin"), { recursive: true });
      fs.copyFileSync(binary, path.join(directory, "bin", executableName(host)));
      fs.chmodSync(path.join(directory, "bin", executableName(host)), 0o755);
    }
    const packed = JSON.parse(npm(["pack", "--ignore-scripts", "--json", "--pack-destination", temporary], { cwd: directory, env }));
    archives.push(path.join(temporary, packed[0].filename));
  }
  const consumer = path.join(temporary, "consumer");
  fs.mkdirSync(consumer);
  fs.writeFileSync(path.join(consumer, "package.json"), '{"private":true}\n');
  npm(["install", "--offline", "--ignore-scripts", "--no-audit", "--no-fund", ...archives], { cwd: consumer, env });
  const installed = path.join(consumer, "node_modules/@tech-byte-frontier/hardgate/bin/hardgate.js");
  const native = path.join(consumer, "node_modules", host, "bin", executableName(host));
  assert.deepEqual(fs.readFileSync(native), fs.readFileSync(binary));
  assert.equal(fs.realpathSync(require(installed).findBinary()), fs.realpathSync(native));
  assert.equal(run(process.execPath, [installed, "--version"], { env }), run(binary, ["--version"], { env }));
  verifyLocalAnalysis(process.execPath, [installed], env);
  console.log(`native installation: ${host} packed install and local analysis passed`);
} finally {
  fs.rmSync(temporary, { recursive: true, force: true });
}
