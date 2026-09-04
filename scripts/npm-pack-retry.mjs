// Classify npm pack failures before bounded registry verification retries.
// Version-resolution failures need independent exact-version evidence.
"use strict";

export function npmErrorText(error) {
  if (typeof error === "string") return error;
  return [error?.code, error?.status, error?.message, error?.stderr, error?.stdout]
    .filter((part) => part !== undefined && part !== null).join("\n");
}

export function isRetryableNpmPackError(error, context = {}) {
  const text = npmErrorText(error);
  if (/\b(?:E401|E403|ENEEDAUTH|EINTEGRITY|EINVALID|EUSAGE|401|403)\b/i.test(text)) return false;
  if (/\bETARGET\b/.test(text)) return context.exactVersionObserved === true;
  return /\b(?:E404|E429|E5\d\d|HTTP(?:\/\d(?:\.\d)?)?\s*(?:404|429|5\d\d)|404|429)\b/i.test(text) ||
    /\b(?:EAI_AGAIN|ECONNRESET|ETIMEDOUT|ECONNREFUSED)\b/i.test(text);
}

export function retryAfterMs(error, now = Date.now()) {
  const value = npmErrorText(error).match(/^retry-after:\s*(.+)$/im)?.[1]?.trim();
  if (!value) return 0;
  if (/^\d+$/.test(value)) return Number(value) * 1000;
  const deadline = Date.parse(value);
  return Number.isFinite(deadline) ? Math.max(0, deadline - now) : 0;
}
