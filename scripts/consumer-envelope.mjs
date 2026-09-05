"use strict";

import { fail } from "./consumer-schema.mjs";

function requireValue(condition, message) {
  if (!condition) fail("report-schema", message);
}

export function validateEnvelope(report, command) {
  requireValue(report.schema_version === 1 && report.command === command, "unsupported report version or command");
  const execution = report.execution;
  requireValue(execution?.command === command, "execution command mismatch");
  requireValue(["repository", "paths", "diff"].includes(execution.scope?.mode) && Array.isArray(execution.scope?.paths), "invalid execution scope");
  requireValue(execution.scope.paths.every((value) => typeof value === "string"), "invalid execution paths");
  requireValue(typeof execution.config?.root === "string" && (execution.config?.path === null || typeof execution.config?.path === "string") && /^[a-f0-9]{64}$/.test(execution.config?.policy_sha256), "invalid policy identity");
  requireValue(Array.isArray(execution.engines), "missing execution engines");
  const ids = new Set();
  for (const engine of execution.engines) {
    validateEngine(engine, ids);
  }
}

function validateEngine(engine, ids) {
  requireValue(typeof engine.id === "string" && !ids.has(engine.id), "invalid or duplicate engine identity");
  ids.add(engine.id);
  requireValue(typeof engine.enabled === "boolean" && typeof engine.selected === "boolean", "invalid engine selection");
  requireValue(["disabled", "skipped", "completed", "failed", "incomplete", "cached"].includes(engine.state), "invalid engine state");
  requireValue(Array.isArray(engine.required_evidence) && engine.required_evidence.every((value) => typeof value === "string"), "invalid engine evidence requirements");
  requireValue(engine.reason === null || typeof engine.reason === "string", "invalid engine reason");
}

export function validateStatus(report, incomplete) {
  const status = incomplete ? "incomplete" : report.passed ? "passed" : "violations";
  const exitCode = { passed: 0, violations: 1, incomplete: 2 }[status];
  if (report.status !== status || report.exit_code !== exitCode) fail("report-status", "report status and exit code disagree with evidence");
}

export function validatePresentation(report) {
  for (const key of ["total", "shown", "omitted", "snippet_bytes"]) {
    requireValue(Number.isSafeInteger(report[key]) && report[key] >= 0, `invalid ${key}`);
  }
  requireValue(typeof report.snippets_truncated === "boolean", "invalid snippet truncation marker");
  requireValue(Array.isArray(report.functions) && Array.isArray(report.diagnostics), "missing report detail arrays");
  if (report.total !== report.summary.total_errors || report.shown !== report.total || report.omitted !== 0) fail("report-status", "consumer report findings are truncated or inconsistent");
}
