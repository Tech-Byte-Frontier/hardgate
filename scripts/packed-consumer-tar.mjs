// Bounded tar/gzip parsing for exact npm package archives.
"use strict";

import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";

export const MAX_ARCHIVE_BYTES = 64 * 1024 * 1024;
export const MAX_BINARY_BYTES = 256 * 1024 * 1024;
const MAX_TOTAL_ARCHIVE_BYTES = 256 * 1024 * 1024;
const MAX_UNPACKED_ARCHIVE_BYTES = 128 * 1024 * 1024;
const MAX_TOTAL_UNPACKED_BYTES = 512 * 1024 * 1024;
const MAX_ARCHIVE_MEMBERS = 1024;
const MAX_MEMBER_BYTES = 64 * 1024 * 1024;
const MAX_TOTAL_MEMBER_BYTES = 256 * 1024 * 1024;

function fail(message) {
  throw new Error(message);
}

export function readBoundedFile(candidate, label, maximum) {
  let fd;
  try {
    fd = fs.openSync(candidate, "r");
    const before = fs.fstatSync(fd);
    if (!before.isFile()) fail(`${label} must be a regular file: ${candidate}`);
    if (before.size > maximum) fail(`${label} exceeds ${maximum} bytes: ${candidate}`);
    const bytes = Buffer.allocUnsafe(before.size);
    let offset = 0;
    while (offset < before.size) {
      const count = fs.readSync(fd, bytes, offset, before.size - offset, offset);
      if (count === 0) fail(`${label} changed while being read: ${candidate}`);
      offset += count;
    }
    const after = fs.fstatSync(fd);
    if (after.size !== before.size || after.mtimeNs !== before.mtimeNs) fail(`${label} changed while being read: ${candidate}`);
    return bytes;
  } finally {
    if (fd !== undefined) fs.closeSync(fd);
  }
}

function tarString(bytes, offset, length) {
  return bytes.subarray(offset, offset + length).toString("utf8").replace(/\0.*$/s, "");
}

function tarNumber(bytes, offset, length) {
  const text = tarString(bytes, offset, length).trim();
  if (!text) return 0;
  const value = Number.parseInt(text, 8);
  if (!Number.isSafeInteger(value) || value < 0) fail(`invalid tar size field: ${JSON.stringify(text)}`);
  return value;
}

function memberPath(header) {
  const name = tarString(header, 0, 100);
  const prefix = tarString(header, 345, 155);
  const member = prefix ? `${prefix}/${name}` : name;
  if (!member || member.startsWith("/") || member.includes("\0") || member.includes("\\")) {
    fail(`npm archive member path is unsafe: ${JSON.stringify(member)}`);
  }
  const segments = member.split("/");
  if (segments.some((segment) => segment.length === 0 || segment === "." || segment === "..")) {
    fail(`npm archive member path is not canonical: ${member}`);
  }
  const normalized = path.posix.normalize(member);
  if (normalized !== member) fail(`npm archive member path is not canonical: ${member}`);
  return normalized;
}

function memberPayload({ header, bytes, archivePath, budget, member, offset }) {
  const size = tarNumber(header, 124, 12);
  if (size > MAX_MEMBER_BYTES) fail(`npm archive member exceeds ${MAX_MEMBER_BYTES} bytes: ${member}`);
  budget.memberBytes += size;
  if (budget.memberBytes > MAX_TOTAL_MEMBER_BYTES) fail(`npm archive members exceed ${MAX_TOTAL_MEMBER_BYTES} bytes: ${archivePath}`);
  const start = offset + 512;
  const end = start + size;
  if (end > bytes.length) fail(`npm archive member exceeds archive length: ${member}`);
  return {
    size,
    start,
    end,
    type: header[156] === 0 ? "0" : String.fromCharCode(header[156]),
    mode: tarNumber(header, 100, 8),
  };
}

function addMember({ entries, entryModes, seen, member, payload, bytes }) {
  if (seen.has(member)) fail(`npm archive contains duplicate member: ${member}`);
  seen.add(member);
  if (payload.type !== "0" && payload.type !== "5") fail(`npm archive member has unsupported type ${JSON.stringify(payload.type)}: ${member}`);
  entryModes.set(member, payload.mode);
  if (payload.type === "0") entries.set(member, Buffer.from(bytes.subarray(payload.start, payload.end)));
  else if (payload.size !== 0) fail(`npm archive directory has contents: ${member}`);
}

function nextOffset(payload) {
  return payload.start + Math.ceil(payload.size / 512) * 512;
}

export function archiveEntries(archiveBytes, archivePath, budget) {
  let bytes;
  try {
    bytes = zlib.gunzipSync(archiveBytes, { maxOutputLength: MAX_UNPACKED_ARCHIVE_BYTES });
  } catch (error) {
    fail(`could not decompress bounded npm archive ${archivePath}: ${error.message}`);
  }
  budget.unpacked += bytes.length;
  if (budget.unpacked > MAX_TOTAL_UNPACKED_BYTES) fail(`npm archives exceed ${MAX_TOTAL_UNPACKED_BYTES} unpacked bytes`);
  const entries = new Map();
  const entryModes = new Map();
  const seen = new Set();
  for (let offset = 0; offset + 512 <= bytes.length; ) {
    const header = bytes.subarray(offset, offset + 512);
    if (header.every((value) => value === 0)) break;
    budget.members += 1;
    if (budget.members > MAX_ARCHIVE_MEMBERS) fail(`npm archives exceed ${MAX_ARCHIVE_MEMBERS} members`);
    const member = memberPath(header);
    const payload = memberPayload({ header, bytes, archivePath, budget, member, offset });
    addMember({ entries, entryModes, seen, member, payload, bytes });
    offset = nextOffset(payload);
  }
  return { entries, entryModes };
}
