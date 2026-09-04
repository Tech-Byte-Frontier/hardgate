// Regression suite for the npm launcher. The focused modules keep each test
// file below repository budgets while sharing only setup and process helpers.
// Run: node tests/npm-wrapper-regression.test.mjs
"use strict";

await import("./npm-wrapper-runtime.test.mjs");
await import("./npm-wrapper-resolution.test.mjs");
await import("./npm-wrapper-contract.test.mjs");

console.log("npm-wrapper-regression.test: OK");
