"use strict";

/**
 * Consumer fixtures deliberately contain no installed dependencies. The
 * runner copies one fixture into a temporary project and uses local command
 * shims to observe Hardgate's resolved package-manager/test command.
 */
function checkSpec(extra = {}) {
  return { expectPass: true, ...extra };
}

function commandCase(id, fixture, mutation, check = {}) {
  return { id, fixture, check: checkSpec(check), mutation };
}

const COMMAND_CASES = [
  ["vite-react-vitest", "vite-react-vitest", { scope: "src", manager: "pnpm", includes: ["test", "App.test.tsx"], requirement: "Vitest framework fallback and React/TSX classification" }],
  ["next-monorepo-package-local", "next-monorepo", { scope: "apps/web/app/page.tsx", manager: "pnpm", includes: ["page.test.tsx"], requirement: "nearest package root and package-local Next workspace tool", cwdSuffix: "/apps/web" }],
  ["jest-fixtures-snapshots", "jest-playwright/jest", { scope: "src/sum.ts", manager: "npm", includes: ["sum.test.ts"], requirement: "Jest test selection while fixture and snapshot files stay visible" }, { minFiles: 8 }],
  ["playwright-suite", "jest-playwright/playwright", { scope: "src/home.ts", manager: "yarn", includes: ["home.spec.ts"], requirement: "Playwright test selection from a Yarn workspace" }],
  ["package-manager-npm", "package-managers/npm", { scope: "src/compute.ts", manager: "npm", includes: ["compute.test.ts"], requirement: "npm package-lock manager detection" }],
  ["package-manager-pnpm", "package-managers/pnpm", { scope: "src/inspect.ts", manager: "pnpm", includes: ["inspect.test.ts"], requirement: "pnpm packageManager and lockfile detection" }],
  ["package-manager-yarn", "package-managers/yarn", { scope: "src/format.ts", manager: "yarn", includes: ["format.test.ts"], requirement: "Yarn lockfile manager detection" }],
  ["package-manager-bun", "package-managers/bun", { scope: "src/scale.ts", manager: "bun", includes: ["scale.test.ts"], requirement: "Bun lockb manager detection" }],
].map(([id, fixture, mutation, check]) => commandCase(id, fixture, mutation, check));

export const CONSUMER_CASES = [
  ...COMMAND_CASES,
  {
    id: "supabase-roles",
    fixture: "supabase",
    check: {
      expectPass: false,
      allowPassWithNoUnsupported: true,
      advisoryIncludes: ["generated file"],
      orchestrationSteps: ["unsupported-source"],
      paths: ["supabase/migrations/001_init.sql", "supabase/seed.sql"],
      minFiles: 9,
      minFunctions: 2,
    },
  },
  {
    id: "greenfield-strict",
    fixture: "greenfield-strict",
    initialize: "strict-agent",
    check: {
      expectPass: false,
      orchestrationSteps: ["coverage-report", "mutation-report"],
      requirement: "strict init must fail closed until coverage and mutation evidence exist",
    },
  },
  {
    id: "legacy-reference-ratchet",
    fixture: "legacy-reference",
    legacy: true,
    check: {
      expectPass: false,
      requireText: /ratchet|baseline|reference/i,
      requirement: "legacy-migration reference-branch ratcheting",
    },
  },
];

export function caseLabel(testCase) {
  return `${testCase.id} (${testCase.fixture})`;
}
