// Pure ABI evidence classifier used by release verification and its offline
// adversarial contract. The target marker is produced by Cargo's build.rs;
// readelf/file evidence remains a defense-in-depth check around that marker.
"use strict";

const GLIBC_MARKERS = /GLIBC_|gnu_get_libc_version|_dl_relocate_static_pie|ld-linux|glibc|\.note\.ABI-tag|NT_GNU_ABI_TAG|GNU ABI tag/i;
const MUSL_INTERPRETER = /ld-musl(?:-[^\s\]]+)?\.so(?:\.[0-9]+)?/i;
const MUSL_SYMBOL = /\b__init_libc\b/;

export function classifyBinaryAbi({ report, programHeaders, symbols, notes = "", abi, targetMarkerValid }) {
  if (!abi) return { ok: true, reason: "non-Linux target" };
  const text = `${report}\n${programHeaders}\n${symbols}\n${notes}`;
  if (abi === "musl") {
    if (GLIBC_MARKERS.test(text)) {
      return { ok: false, reason: "glibc markers are present" };
    }
    const staticBinary = /(?:static(?:-pie)?|statically linked)/i.test(report);
    const muslInterpreter = MUSL_INTERPRETER.test(programHeaders);
    const muslSymbol = MUSL_SYMBOL.test(symbols);
    if (staticBinary) {
      if (!targetMarkerValid) {
        return { ok: false, reason: "static musl binary lacks the exact Cargo target marker" };
      }
      return { ok: true, reason: "exact Cargo target marker and no glibc ABI evidence" };
    }
    if (!muslInterpreter) {
      return { ok: false, reason: "dynamic musl binary lacks a musl interpreter" };
    }
    return {
      ok: true,
      reason: muslSymbol ? "positive musl evidence (__init_libc and interpreter)" : "positive musl evidence (interpreter)",
    };
  }
  if (abi === "gnu" && !/ld-linux|glibc/i.test(text)) {
    return { ok: false, reason: "no glibc ABI marker" };
  }
  return { ok: true, reason: "positive GNU evidence" };
}
