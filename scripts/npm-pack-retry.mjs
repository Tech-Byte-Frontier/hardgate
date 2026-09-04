// Classify npm pack failures before bounded registry verification retries.
// Only explicit not-found propagation and transient transport failures are
// retryable; auth, malformed output, and every other error fail closed.
"use strict";

export function isRetryableNpmPackError(error) {
  const text =
    typeof error === "string"
      ? error
      : [error?.code, error?.status, error?.message, error?.stderr, error?.stdout]
          .filter((part) => part !== undefined && part !== null)
          .join("\n");
  return /\b(?:E404|HTTP(?:\/\d(?:\.\d)?)?\s*404|404)\b/i.test(text) ||
    /\b(?:EAI_AGAIN|ECONNRESET|ETIMEDOUT|ECONNREFUSED)\b/i.test(text);
}
