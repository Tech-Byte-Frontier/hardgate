#!/usr/bin/env node
"use strict";

import { renderHuman, runConsumerMatrix } from "./consumer-runner.mjs";

function parseArgs(argv) {
  const options = { allowPending: false, json: false, keepTemp: false, caseIds: [] };
  let index = 0;
  while (index < argv.length) index = consumeArg(argv, index, options);
  return options;
}

function consumeArg(argv, index, options) {
  const arg = argv[index];
  const flags = { "--allow-pending": "allowPending", "--json": "json", "--keep-temp": "keepTemp" };
  if (flags[arg]) {
    options[flags[arg]] = true;
    return index + 1;
  }
  if (arg === "--help" || arg === "-h") {
    options.help = true;
    return index + 1;
  }
  if (arg === "--case" || arg === "--binary") return consumeValue(argv, index, options, arg);
  throw new Error(`unknown argument: ${arg}`);
}

function consumeValue(argv, index, options, flag) {
  const value = argv[index + 1];
  if (!value) throw new Error(`${flag} requires ${flag === "--case" ? "a fixture id" : "a path"}`);
  if (flag === "--case") options.caseIds.push(value);
  else options.binary = value;
  return index + 2;
}

function printHelp() {
  console.log("Usage: node scripts/check-consumer-matrix.mjs [--json] [--allow-pending] [--case ID] [--binary PATH]");
  console.log("Default mode is CI-strict: pending capabilities fail the command.");
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) {
    printHelp();
    return 0;
  }
  const report = runConsumerMatrix(options);
  if (options.json) console.log(JSON.stringify(report, null, 2));
  else renderHuman(report);
  const blocked = report.summary.fail > 0 || (!options.allowPending && report.summary.pending > 0);
  return blocked ? 1 : 0;
}

try {
  process.exitCode = main();
} catch (error) {
  console.error(`consumer matrix error: ${error.message}`);
  process.exitCode = 2;
}
