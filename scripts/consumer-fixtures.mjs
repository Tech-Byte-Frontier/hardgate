"use strict";

// Offline public check/report contracts. Real specialist mutation execution is
// verified separately; these fixtures do not emulate mutation test runners.
const CHECK_CASES = [
  ["vite-react-vitest", "vite-react-vitest", "React/TSX source and test classification"],
  ["next-monorepo-package-local", "next-monorepo", "Next workspace source inventory"],
  ["jest-fixtures-snapshots", "jest-playwright/jest", "Jest fixtures and snapshots remain visible", 8],
  ["package-manager-npm", "package-managers/npm", "npm project inventory"],
  ["package-manager-pnpm", "package-managers/pnpm", "pnpm project inventory"],
].map(([id, fixture, requirement, minFiles]) => ({
  id, fixture, check: { expectPass: true, expectedExit: 0, requirement, ...(minFiles ? { minFiles } : {}) },
}));

function orchestration(step, command, output) {
  return { step, command, output };
}

export const CONSUMER_CASES = [
  ...CHECK_CASES,
  {
    id: "supabase-roles",
    fixture: "supabase",
    check: {
      expectPass: false,
      expectedExit: 2,
      expectedViolationCount: 2,
      expectedOrchestration: [
        orchestration("unsupported-source", "supabase/migrations/001_init.sql", "File is classified as Migration, but no AST engine supports its extension."),
        orchestration("unsupported-source", "supabase/seed.sql", "File is classified as Migration, but no AST engine supports its extension."),
      ],
      expectedAdvisories: [
        "Classified 2 generated file(s); inventoried without handwritten complexity or clone debt.",
        "generated-freshness evidence: `node supabase/check-generated.mjs` completed successfully.",
      ],
      minFiles: 10,
      minFunctions: 2,
      requirement: "Supabase generated types, edge functions, migrations, and seed SQL",
    },
  },
  {
    id: "greenfield-strict",
    fixture: "greenfield-strict",
    initialize: "strict-agent",
    check: {
      expectPass: false,
      expectedExit: 2,
      expectedViolationCount: 2,
      expectedOrchestration: [
        orchestration("coverage-report", "coverage/lcov.info", "Required coverage report was not found."),
        orchestration("mutation-report", "<not-configured>", "Mutation is enabled, but no report path was provided."),
      ],
      requirement: "strict init must fail closed until coverage and mutation evidence exist",
    },
  },
  {
    id: "legacy-reference-ratchet",
    fixture: "legacy-reference",
    legacy: true,
    check: {
      expectPass: false,
      expectedExit: 1,
      expectedViolationCount: 1,
      expectedComplexity: [
        {
          file: "src/legacy.ts",
          function_name: "legacy",
          metric: "Parameter Count",
          actual: 3,
          limit: 1,
        },
      ],
      legacySummary: {
        reference: "main",
        grandfathered: 0,
        retained: 1,
      },
      requirement: "legacy-migration reference-branch ratcheting",
    },
  },
];

export const CONSUMER_CASE_IDS = Object.freeze(CONSUMER_CASES.map(({ id }) => id));

export function caseLabel(testCase) {
  return `${testCase.id} (${testCase.fixture})`;
}
