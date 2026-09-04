"use strict";

/**
 * Offline consumer contracts.
 *
 * Every fixture is a small, dependency-free project. The runner installs one
 * package-local executable in the copied project and that executable invokes
 * the fixture assertion harness. The harness checks the source bytes and the
 * selected test before returning, so an exit-0 shim can never make a mutant
 * look killed (or make a failed mutation run look green).
 */

function checkSpec(extra = {}) {
  return {
    expectPass: true,
    expectedExit: 0,
    ...extra,
  };
}

function behavior(functionName, args, expected, testNeedle) {
  return { functionName, args, expected, testNeedle };
}

function mutationSpec({
  scope,
  sourcePath,
  testPath,
  manager,
  packageRoot = ".",
  workspaceRoot = ".",
  argv,
  sourceMarker,
  framework,
  behavior,
  requirement,
}) {
  return {
    scope,
    sourcePath,
    testPath,
    manager,
    packageRoot,
    workspaceRoot,
    argv,
    sourceMarker,
    framework,
    behavior,
    requirement,
  };
}

function commandCase(id, fixture, mutation, check = {}) {
  return { id, fixture, check: checkSpec(check), mutation };
}

function orchestration(step, command, output) {
  return { step, command, output };
}

const COMMAND_DEFINITIONS = [
  ["vite-react-vitest", "vite-react-vitest", "src", "src/App.tsx", "src/App.test.tsx", "pnpm", ".", ".", ["test", "--", "src/App.test.tsx"], "value + 1", "vitest", "Vitest framework fallback and React/TSX classification", behavior("increment", [1], 2, "increment(1)).toBe(2)")],
  ["next-monorepo-package-local", "next-monorepo", "apps/web/app/page.tsx", "apps/web/app/page.tsx", "apps/web/app/page.test.tsx", "pnpm", "apps/web", ".", ["test", "--", "app/page.test.tsx"], '"Next:" + name', "vitest", "nearest package root and package-local Next workspace tool", behavior("pageTitle", ["fixture"], "Next:fixture", 'pageTitle("fixture")).toBe("Next:fixture")')],
  ["jest-fixtures-snapshots", "jest-playwright/jest", "src/sum.ts", "src/sum.ts", "tests/sum.test.ts", "npm", ".", ".", ["test", "--", "tests/sum.test.ts"], "left + right", "jest", "Jest test selection while fixture and snapshot files stay visible", behavior("sum", [2, 3], 5, "sum(2, 3)).toBe(5)"), { minFiles: 8 }],
  ["playwright-suite", "jest-playwright/playwright", "src/home.ts", "src/home.ts", "tests/home.spec.ts", "yarn", ".", ".", ["test", "--", "tests/home.spec.ts"], '"Home" + ":"', "playwright", "Playwright test selection from a Yarn workspace", behavior("homeTitle", ["fixture"], "Home: fixture", 'homeTitle("fixture")).toBe("Home: fixture")')],
  ["package-manager-npm", "package-managers/npm", "src/compute.ts", "src/compute.ts", "tests/compute.test.ts", "npm", ".", ".", ["test", "--", "tests/compute.test.ts"], "value + 1", "jest", "npm package-lock manager detection", behavior("compute", [1], 2, "compute(1)).toBe(2)")],
  ["package-manager-pnpm", "package-managers/pnpm", "src/inspect.ts", "src/inspect.ts", "tests/inspect.test.ts", "pnpm", ".", ".", ["test", "--", "tests/inspect.test.ts"], 'value.trim() + ""', "vitest", "pnpm packageManager and lockfile detection", behavior("inspect", [" value "], "value", 'inspect(" value ")).toBe("value")')],
  ["package-manager-yarn", "package-managers/yarn", "src/format.ts", "src/format.ts", "tests/format.test.ts", "yarn", ".", ".", ["test", "--", "tests/format.test.ts"], 'value.toUpperCase() + ""', "jest", "Yarn lockfile manager detection", behavior("format", ["ok"], "OK", 'format("ok")).toBe("OK")')],
  ["package-manager-bun", "package-managers/bun", "src/scale.ts", "src/scale.ts", "tests/scale.test.ts", "bun", ".", ".", ["test", "tests/scale.test.ts"], "value * 2", "bun", "Bun lockb manager detection", behavior("scale", [2], 4, "scale(2)).toBe(4)")],
];

const COMMAND_CASES = COMMAND_DEFINITIONS.map((definition) => {
  const [id, fixture, scope, sourcePath, testPath, manager, packageRoot, workspaceRoot, argv, sourceMarker, framework, requirement, behaviorSpec, check] = definition;
  return commandCase(id, fixture, mutationSpec({ scope, sourcePath, testPath, manager, packageRoot, workspaceRoot, argv, sourceMarker, framework, behavior: behaviorSpec, requirement }), check);
});

export const CONSUMER_CASES = [
  ...COMMAND_CASES,
  {
    id: "supabase-roles",
    fixture: "supabase",
    check: {
      expectPass: false,
      expectedExit: 1,
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
      expectedExit: 1,
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
