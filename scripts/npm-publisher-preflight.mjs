#!/usr/bin/env node
// Authentication checks are separate from permission to publish a package.
"use strict";
import { npmPublisherAuth, validateNpmPublisherToolchain } from "./npm-publisher-auth.mjs";
import { runReleaseProcess } from "./release-process.mjs";

async function main() {
  const args = process.argv.slice(2);
  if (args.length && (args.length !== 2 || args[0] !== "--auth-mode")) {
    throw new Error("usage: npm-publisher-preflight.mjs [--auth-mode token|trusted]");
  }
  const auth = npmPublisherAuth(args[1] ?? process.env.HARDGATE_NPM_AUTH_MODE);
  const npmVersion = await runReleaseProcess("npm", ["--version"], { timeoutMs: 30_000, env: auth.probeEnv });
  validateNpmPublisherToolchain(auth.mode, { nodeVersion: process.version, npmVersion: npmVersion.trim() });
  if (auth.mode === "token") {
    await runReleaseProcess("npm", ["whoami", "--registry=https://registry.npmjs.org"], { timeoutMs: 30_000, env: auth.publishEnv });
    console.log("npm token authentication verified; package write scope is checked by publication");
  } else {
    // npm has no non-mutating OIDC publisher-binding probe. Presence of the
    // runner credentials is a prerequisite, not proof of a registry grant.
    console.log("npm trusted publisher prerequisites verified; package binding is checked by publication");
  }
}

try {
  await main();
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
