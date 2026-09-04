# CLI reference and agent integration

Commands run from the current repository. Hardgate loads `hardgate.toml` when present; without it, the `strict-agent` default bundle is used. A command exits non-zero when its report contains a blocking finding. Advisories remain visible context.

## `hardgate init`

Write a commented configuration template without overwriting an existing file:

```sh
hardgate init --preset strict-agent
hardgate init --preset balanced
hardgate init --preset legacy-migration
hardgate init --preset custom
```

The strict-agent template is the same preset object used by no-config execution. It enables its configured coverage and mutation report policies and includes the configured formatter/linter commands. Supply real evidence and commands before using it as a gate. Balanced disables coverage/mutation report engines. Legacy-migration disables those report engines and enables the static legacy ratchet.

## `hardgate check`

`check` runs static engines and every enabled report/freshness evaluator:

- role-aware file bytes/physical lines and Tree-sitter function budgets;
- suppression and custom-token checks;
- declarative import/call/token invariants;
- bounded token-stream clone detection;
- configured dead-code analysis when enabled (or requested with `--dead-code`);
- LCOV coverage and mutation-report evaluation when those policies are enabled;
- generated-artifact freshness when `[generated].enabled = true`.

```sh
hardgate check
hardgate check --dead-code
hardgate check --coverage-report coverage/lcov.info
hardgate check --format agent
hardgate check --format json
hardgate check --compact
hardgate check --summary
hardgate check src/routes/revenue.ts
```

`--coverage-report` only supplies a path; coverage still has to be enabled. Likewise, mutation report paths are read only when mutation is enabled. Empty or missing required reports, malformed reports, missing source records, and failing generated freshness commands are blocking evidence failures. A disabled engine does not inspect a stale report that happens to exist.

If discovery finds no files, the CLI emits an empty-discovery advisory and continues through every enabled report, freshness, and legacy step. It does not treat the advisory itself as a violation.

### `check --diff`

```sh
hardgate check --diff
hardgate check --diff src/routes/revenue.ts
```

Git status and diff evidence select changed or staged inventory files, including untracked inventory files. A missing Git worktree or malformed Git evidence fails closed. Static findings are scoped to that selection when no legacy ratchet is enabled.

Clone analysis is different: it builds a full repository index of eligible role groups, then reports only clone pairs touching a changed file. This catches a new copy against an unchanged file. Clone fingerprints are content-only and line-independent, so the legacy matcher can preserve identity across a safe rename.

When `[legacy].ratchet = true`, the static pass uses the full current tree so it can compare against the configured reference merge-base. Existing non-worsened static findings (and configured dead-code findings) may be grandfathered as advisories; new or worsened findings remain blocking. Retained findings are annotated with changed-file or changed-hunk context. Enabled coverage is evaluated only on changed executable lines from AST-supported source-role files. Mutation reports and generated freshness remain current blocking evidence; orchestration still requires `--all`.

## `hardgate check --all`

`--all` adds the configured `[orchestration]` format-check, lint, and test commands to `check`. Commands run sequentially from the repository root with a repository-local `node_modules/.bin` available on `PATH`. Hardgate does not discover commands, install tools, or run native mutation as part of `--all`.

```sh
hardgate check --all --format agent
```

An absent command is skipped because it was not configured; a configured command that is empty, unavailable, times out, or exits non-zero is an orchestration finding.

## `hardgate verify`

`verify` is the full-tree evidence gate:

```sh
hardgate verify
hardgate verify --coverage-report coverage/lcov.info \
  --mutation-report reports/stryker-mutation.json
hardgate verify --format agent
hardgate verify --format json --summary
hardgate verify packages/backend
```

It runs full static analysis, enabled coverage/mutation reports, generated freshness, and the configured legacy static/dead-code ratchet. It does not run formatter/linter/test orchestration, Stryker, cargo-mutants, or native AST mutation. Explicitly enabled report/freshness/reference failures are blocking regardless of `gate.strict`; that flag controls static/classification evidence fallback. Empty reports and reports with no recognized outcomes fail closed.

Accepted report inputs are LCOV for coverage and Stryker-shaped, cargo-mutants-shaped, or generic outcome-count JSON for mutation. Mutation scores count killed versus survived; timeout, compile-error, runner-error, and unviable outcomes are integrity findings.

## `hardgate mutate`

Run native AST mutation testing against classified source-role files:

```sh
hardgate mutate --diff
hardgate mutate --scoped src/services/auth.ts --timeout 5 --max-mutants 20
hardgate mutate --scoped src/services/auth.ts \
  --test-cmd 'pnpm test {file}' --format agent
hardgate mutate --json
```

The native runner:

1. selects supported production (`source`) files, never tests or generated/fixture files;
2. resolves one test command per target, unless `--test-cmd` overrides it;
3. executes an unmutated baseline and stops before mutants if that baseline fails;
4. applies bounded binary/boolean AST mutations one at a time;
5. records killed, survived, timeout, compile-error, runner-error, equivalent, and unviable outcomes;
6. restores and verifies original bytes after every mutant.

A scope with no viable mutation points fails. Native mutation is independent of mutation-report ingestion and does not invoke Stryker or cargo-mutants.

### JavaScript/TypeScript command resolution

For JavaScript-family targets (`.js`, `.jsx`, `.mjs`, `.cjs`, `.ts`, `.tsx`, `.mts`, `.cts`), Hardgate walks from the source directory toward the repository root.

1. The nearest `package.json` supplies the package root, optional `packageManager`, scripts, and framework hints. Workspace markers (`pnpm-workspace.yaml`, lockfiles, `.yarnrc.yml`, Bun config, or a `workspaces` field) identify the surrounding workspace boundary.
2. Package-manager precedence is `packageManager` in the nearest manifest, then the nearest lock/config marker, then npm as the fallback. Supported managers are npm, pnpm, Yarn, and Bun.
3. A framework named in a test script takes precedence over manifest/config hints. Otherwise Hardgate uses `jest`, `vitest`, or `playwright` keys in `package.json`, then matching `jest.config.*`, `vitest.config.*`, or `playwright.config.*` files in ancestor directories.
4. The command runs from the package root when a test script exists, from the framework-config root when no script exists, or from the supplied repository root as a final fallback.
5. A matching `<stem>.test.<ext>` or `<stem>.spec.<ext>` is searched beside the source, under `__tests__`/`tests`, and in nested test roots (bounded depth). If no reliable match exists, the full suite is selected.

The generated command uses the detected manager's local binary: `npm test`/`npm run`, `pnpm test`/`pnpm run`, `yarn test`/`yarn <script>`, or `bun test`/`bun run`; direct framework fallback uses `npm exec --offline`, `pnpm exec`, `yarn exec`, or `bun x --no-install`. Jest receives its normal file selector, Vitest receives `run`, and Playwright receives `test`. A project-specific `--test-cmd` is the authoritative override. No resolver path downloads packages; unavailable managers, binaries, or commands are baseline failures.

## `hardgate scan <file>`

Inspect one existing file using role-aware safety and AST metrics:

```sh
hardgate scan src/services/auth.ts
hardgate scan --format json --summary src/services/auth.ts
```

Unsupported inventory formats can still receive applicable file/safety checks but do not produce function metrics. Missing or unreadable paths fail closed.

## `hardgate fmt`

```sh
hardgate fmt
hardgate fmt --check
```

`fmt --check` runs `[orchestration].format_check`; `fmt` runs `format`, falling back to `format_check` when no write command is configured. Commands run from the repository root with local Node binaries available. A configured command failure is blocking for this command.

## Output modes

`check`, `scan`, and `verify` accept `--format terminal|agent|json|compact|summary`, plus `--json`, `--compact`/`--no-snippets`, and `--summary`. `mutate` accepts terminal, agent, or JSON output. JSON is a single machine-readable report; agent output is structured Markdown with actionable locations.

## `hardgate mcp`

Launch the embedded Model Context Protocol server over standard input/output:

```sh
hardgate mcp
```

It accepts newline-delimited or `Content-Length`-framed JSON-RPC. The tool names are:

| Tool | Arguments | Scope |
| --- | --- | --- |
| `hardgate_check` | optional `paths: string[]`, optional `diff: boolean` | Static gate only, including role-aware checks and full-index clone behavior |
| `hardgate_scan_file` | required `path: string` | One-file safety and AST report |
| `hardgate_get_metrics` | required `path: string`, `symbol: string` | Metrics for one named function |

`hardgate_check` is fail-closed: invalid arguments/configuration, missing paths, empty path arrays, empty discovery, unreadable files, parser failures, and Git failures return an explicit failed response. It never runs coverage/mutation reports, generated freshness, dead-code analysis, orchestration, or native mutation. The static report uses the same engine path as the CLI; optional `diff` selects Git-modified scope and full-index clone matching.

Register the stdio server with an MCP-capable client:

```json
{
  "mcpServers": {
    "hardgate": {
      "command": "hardgate",
      "args": ["mcp"]
    }
  }
}
```

For a hook or an agent instruction, describe the exact evidence requested:

```text
After editing, run: hardgate check --diff --format agent
For a full evidence gate, run: hardgate verify --format agent
For configured formatter/linter/test commands, run: hardgate check --all --format agent
For native mutation proof, run: hardgate mutate --format agent
```
