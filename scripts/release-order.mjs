#!/usr/bin/env node
// Refuse a release that could move public "latest" channels backwards.
"use strict";

import { pathToFileURL } from "node:url";

const tagPattern = /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$/;

function parseReleaseTag(tag) {
  const match = tagPattern.exec(tag ?? "");
  if (!match) throw new Error(`invalid v<semver> release tag: ${tag || "<missing>"}`);
  const prerelease = match[4]?.split(".") ?? [];
  for (const identifier of prerelease) {
    if (/^\d+$/.test(identifier) && identifier.length > 1 && identifier.startsWith("0")) {
      throw new Error(`numeric prerelease identifiers cannot have leading zeroes: ${tag}`);
    }
  }
  return {
    tag,
    core: match.slice(1, 4).map(BigInt),
    prerelease,
  };
}

function compareIdentifiers(left, right) {
  const leftNumeric = /^\d+$/.test(left);
  const rightNumeric = /^\d+$/.test(right);
  if (leftNumeric && rightNumeric) {
    const leftNumber = BigInt(left);
    const rightNumber = BigInt(right);
    return leftNumber < rightNumber ? -1 : leftNumber > rightNumber ? 1 : 0;
  }
  if (leftNumeric !== rightNumeric) return leftNumeric ? -1 : 1;
  return left < right ? -1 : left > right ? 1 : 0;
}

function compareCore(left, right) {
  for (let index = 0; index < left.core.length; index += 1) {
    if (left.core[index] < right.core[index]) return -1;
    if (left.core[index] > right.core[index]) return 1;
  }
  return 0;
}

function comparePrereleaseLists(left, right) {
  const sharedLength = Math.min(left.length, right.length);
  for (let index = 0; index < sharedLength; index += 1) {
    const comparison = compareIdentifiers(left[index], right[index]);
    if (comparison !== 0) return comparison;
  }
  return left.length < right.length ? -1 : left.length > right.length ? 1 : 0;
}

function comparePrereleases(left, right) {
  if (left.length === 0 && right.length === 0) return 0;
  if (left.length === 0) return 1;
  if (right.length === 0) return -1;
  return comparePrereleaseLists(left, right);
}

export function compareReleaseTags(leftTag, rightTag) {
  const left = parseReleaseTag(leftTag);
  const right = parseReleaseTag(rightTag);
  const coreComparison = compareCore(left, right);
  return coreComparison || comparePrereleases(left.prerelease, right.prerelease);
}

export function assertReleaseDoesNotRegress(targetTag, latestTag) {
  const comparison = compareReleaseTags(targetTag, latestTag);
  if (comparison < 0 || (comparison === 0 && targetTag !== latestTag)) {
    throw new Error(`target ${targetTag} is not newer than or identical to current latest ${latestTag}`);
  }
}

function requiredArgument(name) {
  const index = process.argv.indexOf(name);
  const value = index < 0 ? undefined : process.argv[index + 1];
  if (!value) throw new Error(`required argument is missing: ${name}`);
  return value;
}

const invokedPath = process.argv[1];
if (invokedPath && import.meta.url === pathToFileURL(invokedPath).href) {
  try {
    const targetTag = requiredArgument("--target-tag");
    const latestTag = requiredArgument("--latest-tag");
    assertReleaseDoesNotRegress(targetTag, latestTag);
    console.log(`release order verified: target ${targetTag}, current latest ${latestTag}`);
  } catch (error) {
    console.error(`release-order: ${error.message}`);
    process.exit(1);
  }
}
