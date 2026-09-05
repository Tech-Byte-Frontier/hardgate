"use strict";

const HASH_PATTERN = /^[0-9a-f]{64}$/;

function parseMetadata(body, fail) {
  try {
    return JSON.parse(body);
  } catch {
    fail("crates.io returned malformed metadata");
  }
}

function assertMetadataVersion(published, version, fail) {
  if (!published || Array.isArray(published) || typeof published !== "object") {
    fail("crates.io metadata has no version object");
  }
  if (published.num !== version) fail("crates.io metadata version does not match the requested version");
  if (published.yanked !== false) fail("the requested crates.io version is yanked");
}

function assertMetadataChecksum(published, expectedSha256, fail) {
  if (published.checksum !== expectedSha256 || !HASH_PATTERN.test(published.checksum)) {
    fail("crates.io checksum does not match the local archive");
  }
}

export function validateMetadata(body, version, expectedSha256, fail) {
  const metadata = parseMetadata(body, fail);
  const published = metadata?.version;
  assertMetadataVersion(published, version, fail);
  assertMetadataChecksum(published, expectedSha256, fail);
  return published;
}

export function validateDefaultMetadata(body, version, fail) {
  const metadata = parseMetadata(body, fail);
  if (metadata?.crate?.max_stable_version !== version) {
    fail("crates.io default stable version does not match the requested version");
  }
  return metadata.crate;
}
