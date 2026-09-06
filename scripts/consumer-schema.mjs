import { validateEnvelope, validateStatus, validatePresentation } from "./consumer-envelope.mjs";
"use strict";

const MAX_DIAGNOSTIC = 4096;

export class ConsumerMatrixError extends Error {
  constructor(code, message) {
    super(message);
    this.name = "ConsumerMatrixError";
    this.code = code;
  }
}

export function fail(code, message) {
  throw new ConsumerMatrixError(code, message);
}

export function bounded(value, limit = MAX_DIAGNOSTIC) {
  const text = String(value ?? "");
  return text.length <= limit ? text : `${text.slice(0, limit)}…[truncated]`;
}

const ENVELOPE_KEYS = ["schema_version", "command", "status", "exit_code", "execution"];
const GATE_KEYS = [
  ...ENVELOPE_KEYS, "functions", "total", "shown", "omitted", "snippet_bytes", "snippets_truncated", "diagnostics",
  "gate_name", "files_scanned", "functions_analyzed", "duration_ms", "passed", "advisories",
  "budget_violations", "suppression_violations", "complexity_violations", "invariant_violations",
  "clone_violations", "coverage_violations", "mutation_violations",
  "orchestration_violations", "summary", "top_files",
];
const SUMMARY_KEYS = [
  "code_findings", "analysis_blockers", "total_errors", "clones", "ast_violations", "complexity", "file_budgets", "suppressions",
  "architecture", "coverage", "mutation", "tool", "files_scanned",
  "functions_analyzed", "files_with_violations", "passed",
];
const SHAPES = {
  budget_violations: ["file", "metric", "actual", "limit", "message"],
  suppression_violations: ["file", "line_number", "token", "line_content", "message"],
  complexity_violations: ["file", "function_name", "line_number", "end_line", "metric", "actual", "limit", "breakdown", "message", "recommendation"],
  invariant_violations: ["file", "line_number", "rule_name", "violation_type", "offending_target", "line_content", "message"],
  clone_violations: ["file_a", "lines_a", "file_b", "lines_b", "tokens", "lines", "fingerprint", "message", "recommendation"],
  coverage_violations: ["file", "function_name", "metric", "actual", "limit", "message", "recommendation"],
  mutation_violations: ["report_file", "metric", "actual", "limit", "message", "recommendation"],
  orchestration_violations: ["step", "command", "exit_code", "output", "recommendation"],
};
const NULLABLE_BY_SHAPE = {
  function_name: new Set([SHAPES.coverage_violations]),
};
const FIELD_VALIDATORS = {
  actual: numberValue,
  limit: numberValue,
  line_number: integerValue,
  end_line: integerValue,
  tokens: integerValue,
  lines: integerValue,
  exit_code: validateExitCode,
  lines_a: validateLinePair,
  lines_b: validateLinePair,
};

function requiredKeys(value, keys, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) fail("report-schema", `${label} must be an object`);
  if (keys.some((key) => !Object.hasOwn(value, key))) {
    fail("report-schema", `${label} must contain ${keys.join(",")}`);
  }
}

function stringValue(value, label, nullable = false) {
  if (nullable && value === null) return;
  if (typeof value !== "string") fail("report-schema", `${label} must be ${nullable ? "a string or null" : "a string"}`);
}

function integerValue(value, label) {
  if (!Number.isSafeInteger(value) || value < 0) fail("report-schema", `${label} must be a non-negative safe integer`);
}

function numberValue(value, label) {
  if (typeof value !== "number" || !Number.isFinite(value)) fail("report-schema", `${label} must be finite`);
}

function booleanValue(value, label) {
  if (typeof value !== "boolean") fail("report-schema", `${label} must be boolean`);
}

function arrayValue(value, label) {
  if (!Array.isArray(value)) fail("report-schema", `${label} must be an array`);
}

function validateLinePair(value, label) {
  arrayValue(value, label);
  if (value.length !== 2 || !value.every((part) => Number.isSafeInteger(part) && part >= 0)) {
    fail("report-schema", `${label} must contain two non-negative integers`);
  }
}

function validateExitCode(value, label) {
  if (value !== null && !Number.isSafeInteger(value)) fail("report-schema", `${label} must be an integer or null`);
}

function validateViolationField(value, key, label, shape) {
  if (key === "breakdown") return;
  if (NULLABLE_BY_SHAPE[key]) return stringValue(value, `${label}.${key}`, NULLABLE_BY_SHAPE[key].has(shape));
  const validator = FIELD_VALIDATORS[key] ?? stringValue;
  return validator(value, `${label}.${key}`);
}

function validateBreakdown(value, label) {
  arrayValue(value, `${label}.breakdown`);
  for (const [index, entry] of value.entries()) {
    const itemLabel = `${label}.breakdown[${index}]`;
    requiredKeys(entry, ["line", "column", "kind", "description", "score"], itemLabel);
    integerValue(entry.line, `${itemLabel}.line`);
    integerValue(entry.column, `${itemLabel}.column`);
    stringValue(entry.kind, `${itemLabel}.kind`);
    stringValue(entry.description, `${itemLabel}.description`);
    integerValue(entry.score, `${itemLabel}.score`);
  }
}

function validateViolation(value, shape, label) {
  requiredKeys(value, shape, label);
  for (const key of shape) validateViolationField(value[key], key, label, shape);
  if (shape.includes("breakdown")) validateBreakdown(value.breakdown, label);
}

function validateGateEnvelope(report) {
  requiredKeys(report, GATE_KEYS, "Hardgate report");
  validateEnvelope(report, "check");
  stringValue(report.gate_name, "gate_name");
  integerValue(report.files_scanned, "files_scanned");
  integerValue(report.functions_analyzed, "functions_analyzed");
  integerValue(report.duration_ms, "duration_ms");
  booleanValue(report.passed, "passed");
  arrayValue(report.advisories, "advisories");
  report.advisories.forEach((item, index) => stringValue(item, `advisories[${index}]`));
}

function validateViolationArrays(report) {
  for (const [field, shape] of Object.entries(SHAPES)) {
    arrayValue(report[field], field);
    report[field].forEach((item, index) => validateViolation(item, shape, `${field}[${index}]`));
  }
}

function validateSummaryShape(report) {
  requiredKeys(report.summary, SUMMARY_KEYS, "summary");
  for (const key of SUMMARY_KEYS) {
    if (key === "passed") booleanValue(report.summary[key], `summary.${key}`);
    else integerValue(report.summary[key], `summary.${key}`);
  }
}

function validateTopFiles(report) {
  arrayValue(report.top_files, "top_files");
  for (const [index, entry] of report.top_files.entries()) {
    const label = `top_files[${index}]`;
    requiredKeys(entry, ["file", "violations"], label);
    stringValue(entry.file, `${label}.file`);
    integerValue(entry.violations, `${label}.violations`);
    if (entry.violations === 0) fail("report-status", `${label} must have a positive violation count`);
  }
}

function fileViolationCount(report) {
  const files = new Set();
  const singleFileFields = ["budget_violations", "suppression_violations", "complexity_violations", "invariant_violations", "coverage_violations"];
  for (const field of singleFileFields) for (const violation of report[field]) files.add(violation.file);
  for (const violation of report.clone_violations) [violation.file_a, violation.file_b].forEach((file) => files.add(file));
  return files.size;
}

function topFileEntries(report) {
  const counts = new Map();
  const add = (file) => counts.set(file, (counts.get(file) ?? 0) + 1);
  const singleFileFields = ["budget_violations", "suppression_violations", "complexity_violations", "invariant_violations", "coverage_violations"];
  for (const field of singleFileFields) for (const violation of report[field]) add(violation.file);
  for (const violation of report.clone_violations) { add(violation.file_a); add(violation.file_b); }
  return [...counts].map(([file, violations]) => ({ file, violations })).sort((a, b) => b.violations - a.violations || a.file.localeCompare(b.file)).slice(0, 10);
}

function expectedGateSummary(report) {
  const counts = Object.fromEntries(Object.entries(SHAPES).map(([field]) => [field, report[field].length]));
  return {
    code_findings: Object.entries(counts).filter(([field]) => field !== "orchestration_violations").reduce((sum, [, count]) => sum + count, 0),
    analysis_blockers: counts.orchestration_violations,
    total_errors: Object.values(counts).reduce((sum, value) => sum + value, 0),
    clones: counts.clone_violations,
    ast_violations: counts.complexity_violations,
    complexity: counts.complexity_violations,
    file_budgets: counts.budget_violations,
    suppressions: counts.suppression_violations,
    architecture: counts.invariant_violations,
    coverage: counts.coverage_violations,
    mutation: counts.mutation_violations,
    tool: counts.orchestration_violations,
    files_scanned: report.files_scanned,
    functions_analyzed: report.functions_analyzed,
    files_with_violations: fileViolationCount(report),
    passed: report.passed,
  };
}

function summaryMatches(actual, expected) {
  for (const key of Object.keys(expected)) if (actual[key] !== expected[key]) return false;
  return true;
}

function validateGateConsistency(report) {
  const total = expectedGateSummary(report).total_errors;
  validateStatus(report, report.orchestration_violations.some((item) => item.exit_code === null || [126, 127].includes(item.exit_code)) || report.coverage_violations.some((item) => ["Missing Source Coverage", "Missing Diff Coverage", "Missing Critical Path", "Coverage Count Overflow"].includes(item.metric)));
  if (report.passed !== (total === 0)) fail("report-status", `passed must equal ${total === 0} when violations are counted`);
  if (JSON.stringify(report.top_files.map(({ file, violations }) => ({ file, violations }))) !== JSON.stringify(topFileEntries(report))) fail("report-status", "top_files is inconsistent with report violations");
  if (!summaryMatches(report.summary, expectedGateSummary(report))) fail("report-status", "summary is inconsistent with report violations");
}

export function validateGateReport(report) {
  validateGateEnvelope(report);
  validateViolationArrays(report);
  validateSummaryShape(report);
  validateTopFiles(report);
  validateGateConsistency(report);
  validatePresentation(report);
  return report;
}

export function parseExactJson(stdout, label = "Hardgate") {
  const text = String(stdout ?? "").trim();
  if (!text) fail("malformed-report", `${label} emitted no JSON report`);
  if (!text.startsWith("{")) fail("malformed-report", `${label} output contains non-JSON text`);
  try {
    const value = JSON.parse(text);
    if (!value || typeof value !== "object" || Array.isArray(value)) fail("malformed-report", `${label} JSON root must be an object`);
    return value;
  } catch (error) {
    if (error instanceof ConsumerMatrixError) throw error;
    fail("malformed-report", `${label} emitted malformed JSON: ${error.message}`);
  }
}
