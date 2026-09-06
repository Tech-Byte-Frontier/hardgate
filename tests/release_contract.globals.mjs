// Static assertions for globally installed release consumers.
"use strict";

import assert from "node:assert/strict";
import { installedConsumers } from "./release_contract.sources.mjs";

const finalConsumerJob = installedConsumers;
const pnpmGlobalConsumer = finalConsumerJob.slice(
  finalConsumerJob.indexOf('pnpm_global="$consumer_tmp/pnpm-global"'),
);
assert.ok(pnpmGlobalConsumer.length > 0, "default pnpm global verification block must exist");
assert.match(
  pnpmGlobalConsumer,
  /pnpm_global_bin_expected="\$pnpm_global\/bin"/,
  "pnpm global verification must use PNPM_HOME/bin as the expected executable directory",
);
assert.match(
  pnpmGlobalConsumer,
  /mkdir -p "\$pnpm_global" "\$pnpm_global_bin_expected"/,
  "pnpm global directories must stay quoted so temporary paths containing spaces remain valid",
);
assert.equal(
  (pnpmGlobalConsumer.match(/PATH="\$pnpm_global_bin_expected:\$manager_path"/g) ?? []).length,
  2,
  "pnpm global install must put its quoted PNPM_HOME/bin path on PATH before add",
);
assert.doesNotMatch(
  pnpmGlobalConsumer,
  /PATH="\$pnpm_global:\$manager_path"/,
  "pnpm global commands must not put PNPM_HOME itself on PATH in place of PNPM_HOME/bin",
);
assert.match(
  pnpmGlobalConsumer,
  /test "\$pnpm_global_bin" = "\$pnpm_global_bin_expected"/,
  "pnpm bin --global output must identify the exact global bin directory",
);
assert.match(
  pnpmGlobalConsumer,
  /pnpm_global_command="\$pnpm_global_bin\/hardgate"/,
  "pnpm global verification must derive the exact binary from the global bin directory",
);
assert.match(
  pnpmGlobalConsumer,
  /test -x "\$pnpm_global_command"/,
  "pnpm global verification must test the exact binary returned by the global bin directory",
);
assert.match(
  pnpmGlobalConsumer,
  /test "\$\(command -v hardgate\)" = "\$1"/,
  "pnpm global verification must resolve command-v to the exact global binary",
);
assert.match(
  pnpmGlobalConsumer,
  /test "\$\(hardgate --version\)" = "\$EXPECTED"/,
  "pnpm global verification must check the exact global binary version",
);
