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

const GATE_KEYS = [
  "gate_name", "files_scanned", "functions_analyzed", "duration_ms", "passed", "advisories",
  "budget_violations", "suppression_violations", "complexity_violations", "invariant_violations",
  "clone_violations", "coverage_violations", "mutation_violations", "dead_code_violations",
  "orchestration_violations", "summary", "top_files",
];
const SUMMARY_KEYS = [
  "total_errors", "clones", "ast_violations", "complexity", "file_budgets", "suppressions",
  "architecture", "coverage", "mutation", "dead_code", "tool", "files_scanned",
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
  dead_code_violations: ["file", "line_number", "symbol", "violation_type", "message", "recommendation"],
  orchestration_violations: ["step", "command", "exit_code", "output", "recommendation"],
};
const MUTATION_KEYS = ["stats", "score", "min_score", "passed", "duration_ms", "results"];
const STATS_KEYS = ["killed", "survived", "timeout", "compile_error", "runner_error", "equivalent", "unviable", "total"];
const MUTANT_KEYS = ["id", "file", "line", "column", "start_byte", "end_byte", "original", "replacement", "description"];
const RESULT_KEYS = ["mutant", "outcome", "duration_ms", "command", "diagnostic", "source_restored"];
const NULLABLE_BY_SHAPE = {
  function_name: new Set([SHAPES.coverage_violations]),
  symbol: new Set([SHAPES.dead_code_violations]),
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

function exactKeys(value, keys, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) fail("report-schema", `${label} must be an object`);
  const actual = Object.keys(value).sort();
  const expected = [...keys].sort();
  if (actual.length !== expected.length || actual.some((key, index) => key !== expected[index])) {
    fail("report-schema", `${label} keys must be exactly ${expected.join(",")}`);
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
  if (key === "line_number" && shape === SHAPES.dead_code_violations && value === null) return;
  if (NULLABLE_BY_SHAPE[key]) return stringValue(value, `${label}.${key}`, NULLABLE_BY_SHAPE[key].has(shape));
  const validator = FIELD_VALIDATORS[key] ?? stringValue;
  return validator(value, `${label}.${key}`);
}

function validateBreakdown(value, label) {
  arrayValue(value, `${label}.breakdown`);
  for (const [index, entry] of value.entries()) {
    const itemLabel = `${label}.breakdown[${index}]`;
    exactKeys(entry, ["line", "column", "kind", "description", "score"], itemLabel);
    integerValue(entry.line, `${itemLabel}.line`);
    integerValue(entry.column, `${itemLabel}.column`);
    stringValue(entry.kind, `${itemLabel}.kind`);
    stringValue(entry.description, `${itemLabel}.description`);
    integerValue(entry.score, `${itemLabel}.score`);
  }
}

function validateViolation(value, shape, label) {
  exactKeys(value, shape, label);
  for (const key of shape) validateViolationField(value[key], key, label, shape);
  if (shape.includes("breakdown")) validateBreakdown(value.breakdown, label);
}

function validateGateEnvelope(report) {
  exactKeys(report, GATE_KEYS, "Hardgate report");
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
  exactKeys(report.summary, SUMMARY_KEYS, "summary");
  for (const key of SUMMARY_KEYS) {
    if (key === "passed") booleanValue(report.summary[key], `summary.${key}`);
    else integerValue(report.summary[key], `summary.${key}`);
  }
}

function validateTopFiles(report) {
  arrayValue(report.top_files, "top_files");
  for (const [index, entry] of report.top_files.entries()) {
    const label = `top_files[${index}]`;
    exactKeys(entry, ["file", "violations"], label);
    stringValue(entry.file, `${label}.file`);
    integerValue(entry.violations, `${label}.violations`);
    if (entry.violations === 0) fail("report-status", `${label} must have a positive violation count`);
  }
}

function fileViolationCount(report) {
  const files = new Set();
  const singleFileFields = ["budget_violations", "suppression_violations", "complexity_violations", "invariant_violations", "coverage_violations", "dead_code_violations"];
  for (const field of singleFileFields) for (const violation of report[field]) files.add(violation.file);
  for (const violation of report.clone_violations) [violation.file_a, violation.file_b].forEach((file) => files.add(file));
  return files.size;
}

function topFileEntries(report) {
  const counts = new Map();
  const add = (file) => counts.set(file, (counts.get(file) ?? 0) + 1);
  const singleFileFields = ["budget_violations", "suppression_violations", "complexity_violations", "invariant_violations", "coverage_violations", "dead_code_violations"];
  for (const field of singleFileFields) for (const violation of report[field]) add(violation.file);
  for (const violation of report.clone_violations) { add(violation.file_a); add(violation.file_b); }
  return [...counts].map(([file, violations]) => ({ file, violations })).sort((a, b) => b.violations - a.violations || a.file.localeCompare(b.file)).slice(0, 10);
}

function expectedGateSummary(report) {
  const counts = Object.fromEntries(Object.entries(SHAPES).map(([field]) => [field, report[field].length]));
  return {
    total_errors: Object.values(counts).reduce((sum, value) => sum + value, 0),
    clones: counts.clone_violations,
    ast_violations: counts.complexity_violations,
    complexity: counts.complexity_violations,
    file_budgets: counts.budget_violations,
    suppressions: counts.suppression_violations,
    architecture: counts.invariant_violations,
    coverage: counts.coverage_violations,
    mutation: counts.mutation_violations,
    dead_code: counts.dead_code_violations,
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
  if (report.passed !== (total === 0)) fail("report-status", `passed must equal ${total === 0} when violations are counted`);
  if (JSON.stringify(report.top_files) !== JSON.stringify(topFileEntries(report))) fail("report-status", "top_files is inconsistent with report violations");
  if (!summaryMatches(report.summary, expectedGateSummary(report))) fail("report-status", "summary is inconsistent with report violations");
}

export function validateGateReport(report) {
  validateGateEnvelope(report);
  validateViolationArrays(report);
  validateSummaryShape(report);
  validateTopFiles(report);
  validateGateConsistency(report);
  return report;
}

function validateMutationShape(report) {
  exactKeys(report, MUTATION_KEYS, "mutation report");
  exactKeys(report.stats, STATS_KEYS, "mutation stats");
  for (const key of STATS_KEYS) integerValue(report.stats[key], `stats.${key}`);
  numberValue(report.score, "score");
  numberValue(report.min_score, "min_score");
  if (report.score < 0 || report.score > 100 || report.min_score < 0 || report.min_score > 100) fail("report-schema", "mutation scores must be between 0 and 100");
  booleanValue(report.passed, "mutation.passed");
  integerValue(report.duration_ms, "mutation.duration_ms");
  arrayValue(report.results, "mutation.results");
}

function validateMutant(mutant, label) {
  exactKeys(mutant, MUTANT_KEYS, label);
  for (const key of ["id", "line", "column", "start_byte", "end_byte"]) integerValue(mutant[key], `${label}.${key}`);
  for (const key of ["file", "original", "replacement", "description"]) stringValue(mutant[key], `${label}.${key}`);
  if (mutant.end_byte < mutant.start_byte) fail("report-schema", `${label} has a reversed byte range`);
}

function validateMutationResult(result, index) {
  const label = `mutation.results[${index}]`;
  exactKeys(result, RESULT_KEYS, label);
  validateMutant(result.mutant, `${label}.mutant`);
  stringValue(result.outcome, `${label}.outcome`);
  if (!["Killed", "Survived", "CompileError", "RunnerError", "Timeout", "Equivalent", "Unviable"].includes(result.outcome)) fail("report-schema", `${label} has an unknown outcome`);
  integerValue(result.duration_ms, `${label}.duration_ms`);
  stringValue(result.command, `${label}.command`);
  stringValue(result.diagnostic, `${label}.diagnostic`);
  booleanValue(result.source_restored, `${label}.source_restored`);
}

function mutationCounts(report) {
  const counts = Object.fromEntries(["Killed", "Survived", "Timeout", "CompileError", "RunnerError", "Equivalent", "Unviable"].map((name) => [name, 0]));
  report.results.forEach((result) => { counts[result.outcome] += 1; });
  return counts;
}

function validateMutationOutcomeCounts(report, counts) {
  const fields = { Killed: "killed", Survived: "survived", Timeout: "timeout", CompileError: "compile_error", RunnerError: "runner_error", Equivalent: "equivalent", Unviable: "unviable" };
  for (const [outcome, field] of Object.entries(fields)) {
    if (counts[outcome] !== report.stats[field]) fail("report-status", `stats.${field} does not match mutation results`);
  }
  return Object.values(fields);
}

function validateMutationTotals(report, fields) {
  const sum = fields.reduce((total, field) => total + report.stats[field], 0);
  if (report.stats.total !== report.results.length || report.stats.total !== sum) fail("report-status", "mutation totals do not match results");
}

function validateMutationScore(report) {
  const viable = report.stats.killed + report.stats.survived;
  const score = viable === 0 ? 0 : (report.stats.killed / viable) * 100;
  if (Math.abs(report.score - score) > 1e-9) fail("report-status", "mutation score is not truthful for killed and survived counts");
  return viable;
}

function validateMutationPassed(report, viable) {
  const passed = viable > 0 && report.score >= report.min_score && report.stats.timeout === 0 && report.stats.compile_error === 0 && report.stats.runner_error === 0 && report.stats.unviable === 0;
  if (report.passed !== passed) fail("report-status", "mutation passed status is inconsistent with score and outcomes");
}

function validateMutationConsistency(report) {
  const fields = validateMutationOutcomeCounts(report, mutationCounts(report));
  validateMutationTotals(report, fields);
  validateMutationPassed(report, validateMutationScore(report));
}

export function validateMutationReport(report) {
  validateMutationShape(report);
  report.results.forEach(validateMutationResult);
  validateMutationConsistency(report);
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
