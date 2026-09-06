// Verify an installed project/global wrapper against the exact release bytes.
"use strict";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
const [rootInput, commandInput, nativeInput, wrapperInput, version, label] = process.argv.slice(2);
const wrapperName = "@tech-byte-frontier/hardgate";
const nativeName = "hardgate-linux-x64";
const fail = (message) => { throw new Error(label + ": " + message); };
const inside = (base, candidate) => {
  const relative = path.relative(base, candidate);
  return relative !== "" && relative !== ".." && !relative.startsWith(".." + path.sep) && !path.isAbsolute(relative);
};
const readJson = (file) => JSON.parse(fs.readFileSync(file, "utf8"));
const resolveFrom = (base, specifier) => fs.realpathSync(createRequire(path.join(base, ".hardgate-resolve.cjs")).resolve(specifier));
const root = fs.realpathSync(rootInput);
const commandEntry = path.resolve(commandInput);
if (!inside(root, commandEntry)) fail("launcher command escaped fresh root");
const command = fs.realpathSync(commandEntry);
const launcherCandidates = [command];
let shim = "";
try { shim = fs.readFileSync(commandEntry, "utf8"); } catch {}
const shimTarget = shim.match(/^# cmd-shim-target=(.+)$/m)?.[1]?.trim();
if (shimTarget) {
  const target = path.resolve(path.dirname(commandEntry), shimTarget);
  launcherCandidates.push(fs.realpathSync(target));
}
const launcher = [...new Set(launcherCandidates)].find((candidate) =>
  candidate.endsWith(`${path.sep}bin${path.sep}hardgate.js`));
if (!launcher) fail("installed command does not resolve to bin/hardgate.js");
const wrapperRoot = path.dirname(path.dirname(launcher));
if (!inside(root, wrapperRoot)) fail("wrapper escaped fresh root");
const wrapperManifestPath = fs.realpathSync(path.join(wrapperRoot, "package.json"));
if (!inside(root, wrapperManifestPath)) fail("wrapper manifest escaped fresh root");
const wrapperManifest = readJson(wrapperManifestPath);
if (wrapperManifest.name !== wrapperName || wrapperManifest.version !== version) fail("wrapper identity mismatch");
if (wrapperManifest.bin?.hardgate !== "bin/hardgate.js") fail("wrapper bin entry mismatch");
if (!fs.readFileSync(launcher).equals(fs.readFileSync(wrapperInput))) fail("wrapper launcher bytes differ");
let nativeManifestPath;
try { nativeManifestPath = resolveFrom(wrapperRoot, nativeName + "/package.json"); } catch {}
if (!nativeManifestPath) fail("native package is not resolvable from checked wrapper");
nativeManifestPath = fs.realpathSync(nativeManifestPath);
if (!inside(root, nativeManifestPath)) fail("native manifest escaped fresh root");
const nativeRoot = path.dirname(nativeManifestPath);
if (!inside(root, nativeRoot)) fail("native package escaped fresh root");
const nativeManifest = readJson(nativeManifestPath);
if (nativeManifest.name !== nativeName || nativeManifest.version !== version) fail("native identity mismatch");
const nativeBinary = fs.realpathSync(path.join(nativeRoot, "bin", "hardgate"));
if (!inside(root, nativeBinary)) fail("native binary escaped fresh root");
if (!fs.readFileSync(nativeBinary).equals(fs.readFileSync(nativeInput))) fail("native bytes differ from dist GNU x64 bytes");
const wrapperModule = createRequire(launcher)(launcher);
if (typeof wrapperModule.findBinary !== "function") fail("wrapper does not export findBinary");
const resolvedNative = wrapperModule.findBinary();
if (!resolvedNative || fs.realpathSync(resolvedNative) !== nativeBinary) fail("wrapper findBinary did not resolve the checked native binary");
process.stdout.write(label + ": verified\n");
