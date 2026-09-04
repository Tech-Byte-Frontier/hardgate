"use strict";

function functionBody(source, functionName) {
  const marker = `export function ${functionName}`;
  const start = source.indexOf(marker);
  if (start < 0) return null;
  const signature = source.slice(start).match(/^export\s+function\s+\w+\s*\(([^)]*)\)/);
  const open = source.indexOf("{", start);
  if (!signature || open < 0) return null;
  let depth = 0;
  for (let index = open; index < source.length; index += 1) {
    if (source[index] === "{") depth += 1;
    if (source[index] === "}") depth -= 1;
    if (depth === 0) return { body: source.slice(open + 1, index), params: signature[1] };
  }
  return null;
}

function parameterNames(parameters) {
  return parameters.split(",").map((parameter) => parameter.trim().replace(/:.*/, "").trim()).filter(Boolean);
}

function execute(source, behavior) {
  const extracted = functionBody(source, behavior.functionName);
  if (!extracted) return { actual: null, passed: false, reason: "behavior function was not found" };
  try {
    const evaluate = new Function(...parameterNames(extracted.params), extracted.body);
    const actual = evaluate(...behavior.args);
    return { actual, passed: Object.is(actual, behavior.expected), reason: null };
  } catch (error) {
    return { actual: null, passed: false, reason: `behavior execution failed: ${error.message}` };
  }
}

export function evaluateBehavior(source, test, behavior) {
  if (typeof behavior.testNeedle !== "string" || !test.includes(behavior.testNeedle)) {
    return withFailureEvidence({ actual: null, passed: false, reason: "selected test assertion was not found" });
  }
  return withFailureEvidence(execute(source, behavior));
}

function withFailureEvidence(result) {
  if (!result.passed && process.env.CONSUMER_LOG) {
    process.stderr.write(`AssertionError: ${result.reason ?? "consumer behavior mismatch"}\n`);
  }
  return result;
}
