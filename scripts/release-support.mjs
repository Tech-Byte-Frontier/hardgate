// Shared release-script argument and project metadata helpers.
"use strict";

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

export const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

// RFC 4122's URL namespace keeps deterministic release identifiers separate
// from UUIDv5 values derived in DNS, OID, or X.500 namespaces.
const UUID_URL_NAMESPACE = Buffer.from("6ba7b8119dad11d180b400c04fd430c8", "hex");

export function uuidV5(name) {
  const bytes = crypto.createHash("sha1").update(UUID_URL_NAMESPACE).update(name, "utf8").digest().subarray(0, 16);
  bytes[6] = (bytes[6] & 0x0f) | 0x50;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = bytes.toString("hex");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

export function option(name, fallback, argv = process.argv) {
  const index = argv.indexOf(name);
  if (index >= 0) return argv[index + 1];
  return argv.find((value) => value.startsWith(`${name}=`))?.slice(name.length + 1) ?? fallback;
}

export function readCargoVersion(root = projectRoot) {
  const cargoToml = fs.readFileSync(path.join(root, "Cargo.toml"), "utf8");
  return cargoToml.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
}

export function runCommand(prefix, command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: "utf8", ...options });
  if (result.status !== 0) {
    throw new Error(`${prefix}: ${command} failed: ${result.error?.message ?? result.stderr}`);
  }
  return result.stdout;
}

export function archiveMemberMode(listing, member) {
  const line = listing.split("\n").find((entry) => entry.trim().endsWith(` ${member}`));
  return line?.trim().split(/\s+/, 1)[0];
}

export function isExecutableMode(mode) {
  return Boolean(mode && mode.length === 10 && mode.startsWith("-") && [mode[3], mode[6], mode[9]].some((value) => /[xstST]/.test(value)));
}
