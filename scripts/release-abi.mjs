// Positive ABI evidence for the supported Linux x64 GNU release artifact.
"use strict";

export function classifyBinaryAbi({ report, programHeaders, symbols, notes = "", abi }) {
  if (abi === "darwin") return { ok: /Mach-O 64-bit/.test(report), reason: "expected a 64-bit Mach-O executable" };
  if (abi === "msvc") return { ok: /PE32\+ executable/.test(report), reason: "expected a 64-bit Windows PE executable" };
  if (abi !== "gnu") return { ok: false, reason: "unsupported native ABI" };
  const text = `${report}\n${programHeaders}\n${symbols}\n${notes}`;
  if (/ld-musl|__init_libc/.test(text)) return { ok: false, reason: "musl ABI evidence is unsupported" };
  if (!/ELF 64-bit/.test(report) || !/ld-linux|glibc/i.test(text)) {
    return { ok: false, reason: "no positive ELF/glibc ABI evidence" };
  }
  const versions = [...text.matchAll(/GLIBC_(\d+)\.(\d+)(?:\.(\d+))?/g)];
  if (versions.some(([, major, minor, patch]) => Number(major) > 2 || (Number(major) === 2 && (Number(minor) > 39 || (Number(minor) === 39 && Number(patch ?? 0) > 0))))) {
    return { ok: false, reason: "artifact requires glibc newer than the supported 2.39 baseline" };
  }
  return { ok: true, reason: "positive GNU evidence" };
}
