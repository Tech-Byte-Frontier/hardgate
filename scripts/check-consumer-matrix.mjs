#!/usr/bin/env node
"use strict";

import { renderHuman, runConsumerMatrix } from "./consumer-runner.mjs";

const SIMPLE_FLAGS = {
  "--json": (options) => { options.json = true; },
  "--keep-temp": (options) => { options.keepTemp = true; },
  "--help": (options) => { options.help = true; },
  "-h": (options) => { options.help = true; },
  "--allow-pending": () => { throw new Error("--allow-pending is not supported; pending capabilities are blocking"); },
};
const VALUE_FLAGS = { "--case": "a fixture id", "--binary": "a path" };

function readArgument(argv, index, options) {
  const arg = argv[index];
  const simple = SIMPLE_FLAGS[arg];
  if (simple) { simple(options); return index; }
  const valueLabel = VALUE_FLAGS[arg];
  if (!valueLabel) throw new Error(`unknown argument: ${arg}`);
  const value = argv[index + 1];
  if (!value) throw new Error(`${arg} requires ${valueLabel}`);
  if (arg === "--case") options.caseIds.push(value); else options.binary = value;
  return index + 1;
}

function parseArgs(argv) {
  const options = { json: false, keepTemp: false, caseIds: [] };
  for (let index = 0; index < argv.length; index += 1) index = readArgument(argv, index, options);
  return options;
}

function printHelp() {
  console.log("Usage: node scripts/check-consumer-matrix.mjs [--json] [--case ID] [--binary PATH]");
  console.log("The matrix is offline and strict; process, report, evidence, and pending failures block CI.");
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) { printHelp(); return 0; }
  const report = runConsumerMatrix(options);
  if (options.json) console.log(JSON.stringify(report, null, 2)); else renderHuman(report);
  return report.summary.fail === 0 && report.summary.pending === 0 ? 0 : 1;
}

try {
  process.exitCode = main();
} catch (error) {
  const code = error?.code ? `[${error.code}] ` : "";
  console.error(`consumer matrix error: ${code}${error.message}`);
  process.exitCode = 2;
}
