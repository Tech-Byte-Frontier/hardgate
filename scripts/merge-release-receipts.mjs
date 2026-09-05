// Merge independently written release receipt checkpoints without widening
// their evidence or bypassing the receipt schema.
"use strict";

import path from "node:path";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";
import {
  completeFromChannels,
  clone,
  fail,
  REQUIRED_CHANNELS,
  validateIdentity,
  validateReceipt,
} from "./release-receipt-validation.mjs";
import {
  readReceipt as readReceiptSync,
  writeReceiptAtomicSync,
} from "./release-receipt.mjs";

function isPrefix(prefix, history) {
  return prefix.length <= history.length && prefix.every((event, index) => isDeepStrictEqual(event, history[index]));
}

function mergeChannelHistory(receipts, channelName) {
  let selected = receipts[0].channels[channelName];
  for (const receipt of receipts.slice(1)) {
    const candidate = receipt.channels[channelName];
    if (isDeepStrictEqual(selected.events, candidate.events)) continue;
    if (isPrefix(selected.events, candidate.events)) selected = candidate;
    else if (!isPrefix(candidate.events, selected.events)) fail(`channel ${channelName} has divergent event histories`);
  }
  return clone(selected);
}

export function mergeReceipts(receipts, expectedIdentity) {
  if (!Array.isArray(receipts) || receipts.length === 0) fail("receipts must be a non-empty array");
  const identity = validateIdentity(expectedIdentity, "expectedIdentity");
  for (const receipt of receipts) validateReceipt(receipt, identity);
  const merged = clone(receipts[0]);
  for (const channelName of REQUIRED_CHANNELS) merged.channels[channelName] = mergeChannelHistory(receipts, channelName);
  merged.complete = completeFromChannels(merged.channels);
  validateReceipt(merged, identity);
  return merged;
}

function setPathOption(options, argv, index, argument) {
  const key = argument === "--expected" ? "expectedPath" : "outputPath";
  if (options[key] !== null) fail(`${argument} was specified more than once`);
  const value = argv[index + 1];
  if (!value || value.startsWith("-")) fail(`${argument} requires a value`);
  options[key] = value;
  return index + 1;
}

function parseArgument(options, argv, index) {
  const argument = argv[index];
  if (argument === "--require-complete") {
    if (options.requireComplete) fail("--require-complete was specified more than once");
    options.requireComplete = true;
    return index;
  }
  if (argument === "--expected" || argument === "--output") return setPathOption(options, argv, index, argument);
  if (argument.startsWith("-")) fail(`unknown option ${argument}`);
  options.inputs.push(argument);
  return index;
}

function parseArguments(argv) {
  const options = { expectedPath: null, outputPath: null, requireComplete: false, inputs: [] };
  for (let index = 0; index < argv.length; index += 1) index = parseArgument(options, argv, index);
  if (options.expectedPath === null) fail("--expected requires a value");
  if (options.outputPath === null) fail("--output requires a value");
  if (options.inputs.length === 0) fail("at least one channel receipt is required");
  return options;
}

function completedChannels(receipt) {
  return REQUIRED_CHANNELS.filter((channel) => receipt.channels[channel].state === "default_consumer_verified").length;
}

function run(argv) {
  const options = parseArguments(argv);
  const expected = readReceiptSync(options.expectedPath);
  const receipts = options.inputs.map((input) => readReceiptSync(input, expected.identity));
  const merged = mergeReceipts(receipts, expected.identity);
  writeReceiptAtomicSync(options.outputPath, merged, expected.identity);
  const count = completedChannels(merged);
  const status = merged.complete ? "complete" : "incomplete";
  console.log(`release receipt: ${count}/${REQUIRED_CHANNELS.length} channels complete (${status})`);
  if (options.requireComplete && !merged.complete) process.exitCode = 1;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    run(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  }
}
