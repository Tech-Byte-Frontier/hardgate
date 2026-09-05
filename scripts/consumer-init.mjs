"use strict";
import fs from "node:fs";
import path from "node:path";
import { runProcess, processFailure } from "./consumer-process.mjs";
import { fail, parseExactJson } from "./consumer-schema.mjs";

export function initializeFixture(binary, root, preset) {
  const result = runProcess({ binary, args: ["init", "--preset", preset], cwd: root });
  const error = processFailure(result, 0, "init");
  if (error) fail(error[0], error[1]);
  const configPath = path.join(root, "hardgate.toml");
  if (!fs.existsSync(configPath) || !fs.statSync(configPath).isFile()) fail("fixture-init", "hardgate init did not write hardgate.toml");
  const effective = runProcess({ binary, args: ["config", "--format", "json"], cwd: root });
  const configError = processFailure(effective, 0, "config");
  if (configError) fail(configError[0], configError[1]);
  const config = parseExactJson(effective.stdout, "config").effective;
  if (config?.gate?.preset !== "strict-agent" || config?.gate?.strict !== true || config.budgets?.excludes?.includes("tests/**")) fail("fixture-init", "strict init wrote an unexpected effective policy");
}
