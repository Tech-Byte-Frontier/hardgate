// Apply the GitHub default-channel promotion after receipt gates pass.
"use strict";

import { promoteVerifiedChannel } from "./channel-promotion.mjs";
import {
  assertExactKeys,
  assertPlainObject,
  CHANNELS,
  clone,
  RECEIPT_STATES,
  REQUIRED_CHANNELS,
  validateReceipt,
} from "./release-receipt-validation.mjs";
import { recordTransition } from "./release-receipt.mjs";

const GITHUB_CHANNEL = CHANNELS.githubAssets;
const EXACT_STATE = RECEIPT_STATES.indexOf("exact_consumer_verified");
const PROMOTED_STATE = RECEIPT_STATES.indexOf("promoted");

function fail(message) {
  throw new Error(`GitHub promotion: ${message}`);
}

function assertRequest(value) {
  assertPlainObject(value, "request");
  assertExactKeys(value, ["receipt", "policy"], "request");
  validateReceipt(value.receipt);
  return value;
}

function stateAtLeast(receipt, channel, minimum) {
  const state = receipt.channels[channel].state;
  return RECEIPT_STATES.indexOf(state) >= minimum;
}

function assertReceiptGate(receipt) {
  if (REQUIRED_CHANNELS.some((channel) => !stateAtLeast(receipt, channel, EXACT_STATE))) {
    fail("all required channels must reach exact_consumer_verified before GitHub promotion");
  }
  const npmChannels = [...CHANNELS.npmPlatforms, CHANNELS.npmWrapper];
  if (npmChannels.some((channel) => !stateAtLeast(receipt, channel, PROMOTED_STATE))) {
    fail("all npm channels must reach promoted before GitHub promotion");
  }
}

function promotionEvidence(receipt) {
  return {
    version: receipt.identity.version,
    source_sha: receipt.identity.source_sha,
    archives: clone(receipt.identity.archives),
  };
}

export function validateGithubPromotionReceipt(receipt) {
  validateReceipt(receipt);
  assertReceiptGate(receipt);
  return receipt;
}

export async function promoteGithubChannel(request, operations) {
  const checked = assertRequest(request);
  assertReceiptGate(checked.receipt);
  const helperRequest = {
    version: checked.receipt.identity.version,
    exactConsumerVerified: true,
    policy: checked.policy,
  };
  const result = await promoteVerifiedChannel(helperRequest, operations);
  const current = checked.receipt.channels[GITHUB_CHANNEL].state;
  if (current === "exact_consumer_verified") {
    recordTransition(checked.receipt, {
      channel: GITHUB_CHANNEL,
      from: current,
      to: "promoted",
      evidence: promotionEvidence(checked.receipt),
    });
    return { publication: result.publication, state: "promoted", receipt: checked.receipt };
  }
  return { publication: result.publication, state: current, receipt: checked.receipt };
}

export { GITHUB_CHANNEL };
