# System architecture

Hardgate is a local Rust CLI composed of configuration loading, inventory discovery, role classification, independent policy engines, evidence evaluators, optional command orchestration, and report renderers. Each engine owns its inputs and exclusions; a report keeps blocking findings and advisories visible together.

```text
CLI / MCP (stdio)
       |
       v
config (preset + presence merge) -> discovery -> role classification
       |
       +--> file/anti-gaming safety
       +--> Tree-sitter complexity
       +--> declarative invariants
       +--> role-group clone index
       +--> optional dead-code analysis
       +--> enabled LCOV/mutation evidence
       +--> generated freshness
       +--> optional formatter/linter/test orchestration (--all)
       |
       v
  report -> terminal / agent Markdown / JSON / compact / summary
```

## Configuration and discovery

`HardgateConfig::load_or_default` loads `hardgate.toml`, or the `strict-agent` preset when no file exists. `hardgate init --preset strict-agent` serializes the same object. For non-custom presets, a TOML section/key overlays the preset only when that key is present. Explicit `false` and empty values remain explicit; omitted keys retain preset values.

The walker inventories source and text/data extensions while pruning dependency/build directories (`node_modules`, `target`, `dist`, `build`, `vendor`, `.venv`, `venv`, `__pycache__`). User budget or clone exclusions are not discovery pruning. Those excluded files remain visible to classification and other engines and produce an advisory from the owning engine; dead-code exclusions are local to that analyzer and silent.

Each file becomes a `ClassifiedFile` with a role and AST-support flag. Ordered custom classification rules run before built-ins; vendor/build pruning cannot be overridden by a user rule.

## Role policy

The first-class policy roles are source, test, generated, fixture, and migration. Each has independent severity, file/function thresholds, and clone settings; native mutation is source-role-only and source eligibility is configurable:

- **Source:** safety, invariants, AST complexity when supported, role-group clones, and native mutation targets.
- **Test:** safety, invariants, AST complexity, role-group clones, never native mutation.
- **Generated:** inventoried and surfaced as generated; ignored for handwritten complexity/clone debt by default.
- **Fixture:** safety and role-group clone analysis; no complexity by default.
- **Migration:** safety checks; no native mutation or clone analysis by default.

Configuration and documentation files remain inventoried for applicable policy, vendor files are pruned, and unknown files can fail when `gate.enforce_classified_sources` is enabled. A role severity of `error`, `warning`, or `ignore` determines whether a role finding blocks, becomes an advisory, or is omitted. Static evidence without a role override falls back to `gate.strict`, except enforced unknown-role classification gaps, which are always blocking.

### Node and Supabase boundaries

The JavaScript-family parser set includes `.js`, `.jsx`, `.mjs`, `.cjs`,
`.ts`, `.tsx`, `.mts`, and `.cts` for source and test roles. Built-in
classification treats `supabase/database.types.ts` and
`supabase/schema.gen.ts` as generated, `supabase/functions/**/*.ts` as source,
and Supabase migrations/seeds (`supabase/migrations/**/*.sql`,
`supabase/seed.sql`, `supabase/seed.ts`, and `*.migration.sql`/`*.seed.sql`)
as migration. SQL migration/seed files are inventoried and receive applicable
safety checks but have no AST parser; `supabase/seed.ts` is migration-role and
has TypeScript parser support, while migration policy still does not apply
ordinary source/test complexity or native mutation. Under the default strict
migration policy, parser-unsupported migration files produce a blocking
`unsupported-source` finding. A custom classification rule may assign a
different role, but it does not add a SQL parser. TOML/JSON
configuration such as `supabase/config.toml` remains configuration inventory,
not executable source.

## Static engines

### Safety and budgets

File budgets measure raw bytes and physical lines. Function budgets use Tree-sitter metrics for Rust, JavaScript, TypeScript, TSX, Python, and Go: cyclomatic/cognitive complexity, Halstead difficulty, ABC, parameters, statements, function lines, and nesting. Parse and read failures stay evidence failures; they are not converted to zero functions.

The anti-gaming scanner recognizes common suppression directives and configured forbidden tokens in safety-checked roles. There is no inline approval path.

### Invariants

Invariant rules compile `from`/`exclude` globs and inspect import strings, call names, and literal tokens line by line. They intentionally do not resolve modules or type-check a project.

The invariant engine is enabled by default; an empty rule list is a no-op, while `enforce = false` disables it explicitly.

### Clones

The clone detector tokenizes non-comment code, normalizes literal values, indexes bounded windows with a rolling hash, verifies token sequences, and coalesces adjacent matches. Source, test, and fixture files are analyzed in independent role groups with independently configurable thresholds. In diff mode the index contains the full discovered repository; only pairs touching Git-changed/staged files or explicitly selected existing paths are emitted.

Each violation carries a stable fingerprint computed from normalized token kinds. Paths and physical line numbers are excluded from that fingerprint. Git rename lineage can therefore map a current path to its baseline path without changing clone identity.

## Evidence engines

### Coverage

When `[coverage].enabled` is true, the LCOV parser evaluates global line/function/branch floors, function CRAP scores, critical paths, and source records. CRAP scoring requires at least one LCOV `DA` entry inside the parsed function range: a zero-hit entry remains uncovered, while a range with no executable line evidence is omitted as target- or `cfg`-excluded. In `check --diff`, changed Git lines are filtered to AST-supported source-role files and only changed executable lines are scored. Missing, empty, unreadable, malformed, or incomplete required coverage evidence is blocking regardless of `gate.strict`.

### Mutation reports

When `[mutation].enabled` is true, `check` and `verify` evaluate Stryker-shaped, cargo-mutants-shaped, or generic outcome-count JSON. Scores use killed divided by killed plus survived. Empty reports/outcomes, missing paths, parse errors, and no viable outcomes fail. Timeout, compile-error, runner-error, and unviable outcomes are integrity findings; report evidence is always current and never ratcheted.

### Generated freshness

`[generated]` is a separate command-backed evidence engine. When enabled, a non-empty `freshness_command` runs in `check` and `verify` with its own timeout. Excluding generated files from file budgets does not disable freshness. Missing command, timeout, runner failure, or non-zero exit is current blocking evidence and is outside legacy ratchet matching.

### Legacy adoption

With `[legacy].ratchet = true`, Hardgate resolves the configured Git reference and merge base, loads a baseline snapshot, runs the static gate (plus configured dead code), and compares current findings. Existing non-worsened static debt can be grandfathered as advisories. New or worsened findings with effective role severity `error` remain blocking; `warning` findings remain advisories and `ignore` findings are omitted. Retained findings are annotated with changed-file or changed-hunk context. Budget, suppression, complexity, invariant, clone, and dead-code findings participate. Coverage, mutation, generated freshness, and orchestration findings remain current blocking evidence and are not grandfathered. Missing reference, merge base, snapshot, or baseline analysis is a blocking evidence failure.

## Command boundaries

- `check`: static engines, enabled reports, freshness, and optional configured dead code.
- `check --diff`: Git-changed/staged static scope by default, with explicit existing paths added to static/clone selection and full-index clone matching; with a legacy ratchet, static/clone analysis uses the full current selected scope (whole tree when no paths are supplied). LCOV always intersects actual changed executable lines.
- `check --all`: `check` plus configured formatter/linter/test orchestration.
- `verify`: full static/dead-code tree and configured evidence by default; optional path filters scope only current static/dead-code inventory and coverage source matching, while mutation reports and freshness remain configured/full. The ratchet loads the full configured reference snapshot but compares only selected current static/dead-code findings; no orchestration or native mutation.
- `mutate`: when `[mutation].enabled = true`, native unmutated baseline and AST mutants; when disabled, prints a note and succeeds without target discovery or execution; no report ingestion.

Orchestration commands run sequentially from the repository root; a local `node_modules/.bin` is prepended to `PATH`. An unconfigured command is not inferred. A configured command that is empty, unavailable, times out, or exits non-zero yields an orchestration finding.

## Native mutation and JavaScript resolution

Native mutation is source-role only and is available to source builds on Linux
and macOS through the target-OS cfg. On other operating systems it fails closed
before baseline or source writes because robust process-group cleanup and
atomic restoration are not available. When `[mutation].enabled = false`, the
command exits successfully without target discovery or execution. When enabled,
it resolves a test command per file, runs a passing unmutated baseline, mutates
one AST point at a time, classifies outcomes, and verifies byte-for-byte
restoration. A failed baseline or zero viable mutants fails before a green
result. The prebuilt, npm, and shell-installer release contract remains exactly
six x64/arm64 glibc/musl/macOS targets (Linux x64/arm64 glibc and musl, macOS
x64/arm64); that distribution matrix does not constrain source builds.

After an explicit scope is validated, a `mutate --diff` run (including
`--scoped`) with no changed production source is a successful no-op. Missing,
invalid, unsupported, or non-source explicit scopes fail closed; only a non-diff
unrestricted or scoped run with no eligible source target fails.

For JS/TS files, the resolver validates every encountered `package.json`; a
malformed or unreadable manifest fails closed, with `--test-cmd` as the explicit
override. Only validated `workspaces` declarations or a valid
`pnpm-workspace.yaml` `packages` list establish a workspace; lockfiles and
manager config are hints for npm, pnpm, Yarn, or Bun selection, never workspace
proof. A child package's `test` script wins, one unambiguous `test:*` script is
allowed, and multiple `test:*` scripts fail closed. A reliable child-local
manifest, framework-config, or script signal takes precedence over an
enclosing workspace-root script; that root script is used only when the child
has no local script or reliable local manifest/config/script signal. Framework
selectors are inferred
only from unambiguous recognized commands or hints;
ambiguous/composed cases use the full suite. A matching sibling or nested
`<stem>.test|spec>` file is selected when reliable, otherwise the full suite
runs. Manager-local `test`/`run`/`exec` commands are used, with
`npm exec --offline` and `bun x --no-install` preventing runtime installation.
`--test-cmd` overrides this resolver. Framework selection uses only validated
manifest fields, known config filenames, and unambiguous script commands; it
does not scan dependency packages.

## Reports and MCP

The report aggregator records scan/function counts, violations, advisories, and duration, then renders terminal, agent Markdown, JSON, compact, or summary output. CLI empty discovery is an advisory while enabled evidence still runs; malformed or missing required evidence remains blocking. The MCP check surface rejects empty discovery before producing a report.

The MCP server uses newline-delimited or `Content-Length`-framed JSON-RPC over stdio. Its tools are `hardgate_check(paths?, diff?)`, `hardgate_scan_file(path)`, and `hardgate_get_metrics(path, symbol)`. `hardgate_check` routes through the static gate only; `diff` defaults to Git-changed/staged inventory, explicit existing paths add to static/clone selection, and clone matching uses the full repository index. MCP does not run coverage, mutation, freshness, dead code, orchestration, or native mutation. Invalid arguments/configuration, missing paths, empty scopes/discovery, and Git failures are outer tool errors. Read/parse failures are report-level Hardgate `Failed` findings whose effective role severity makes them errors, advisories, or omitted findings (`error`, `warning`, or `ignore`). For `hardgate_scan_file`, a read failure is an outer tool error while parse/static findings remain in its per-file report; `hardgate_get_metrics` reports read or missing-symbol errors explicitly.

## Build identity

Release packaging writes `BUILD-METADATA.json` with the binary name, numeric
version, Cargo target triple, npm package, and full source commit. Binaries
embed `hardgate-target:<target>` and `--version` emits exactly
`hardgate VERSION (COMMIT)`; checksum, metadata, target, and identity checks
are part of the release/archive verification contract.
