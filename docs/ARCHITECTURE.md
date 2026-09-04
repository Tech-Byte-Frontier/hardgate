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

The walker inventories source and text/data extensions while pruning dependency/build directories (`node_modules`, `target`, `dist`, `build`, `vendor`, `.venv`, `venv`, `__pycache__`). User budget or clone exclusions are not discovery pruning. Excluded files remain visible to classification and other engines and produce an advisory from the owning engine.

Each file becomes a `ClassifiedFile` with a role and AST-support flag. Ordered custom classification rules run before built-ins; vendor/build pruning cannot be overridden by a user rule.

## Role policy

The first-class policy roles are source, test, generated, fixture, and migration. Each has independent severity, file/function thresholds, clone settings, and native mutation eligibility:

- **Source:** safety, invariants, AST complexity when supported, role-group clones, and native mutation targets.
- **Test:** safety, invariants, AST complexity, role-group clones, never native mutation.
- **Generated:** inventoried and surfaced as generated; ignored for handwritten complexity/clone debt by default.
- **Fixture:** safety and role-group clone analysis; no complexity by default.
- **Migration:** safety checks; no native mutation or clone analysis by default.

Configuration and documentation files remain inventoried for applicable policy, vendor files are pruned, and unknown files can fail when `gate.enforce_classified_sources` is enabled. A role severity of `error`, `warning`, or `ignore` determines whether a role finding blocks, becomes an advisory, or is omitted. Static evidence without a role override falls back to `gate.strict`.

## Static engines

### Safety and budgets

File budgets measure raw bytes and physical lines. Function budgets use Tree-sitter metrics for Rust, JavaScript, TypeScript, TSX, Python, and Go: cyclomatic/cognitive complexity, Halstead difficulty, ABC, parameters, statements, function lines, and nesting. Parse and read failures stay evidence failures; they are not converted to zero functions.

The anti-gaming scanner recognizes common suppression directives and configured forbidden tokens in safety-checked roles. There is no inline approval path.

### Invariants

Invariant rules compile `from`/`exclude` globs and inspect import strings, call names, and literal tokens line by line. They intentionally do not resolve modules or type-check a project.

The invariant engine is enabled by default; an empty rule list is a no-op, while `enforce = false` disables it explicitly.

### Clones

The clone detector tokenizes non-comment code, normalizes literal values, indexes bounded windows with a rolling hash, verifies token sequences, and coalesces adjacent matches. Source, test, and fixture files are analyzed in independent role groups with independently configurable thresholds. In diff mode the index contains the full discovered repository; only pairs touching a changed file are emitted.

Each violation carries a stable fingerprint computed from normalized token kinds. Paths and physical line numbers are excluded from that fingerprint. Git rename lineage can therefore map a current path to its baseline path without changing clone identity.

## Evidence engines

### Coverage

When `[coverage].enabled` is true, the LCOV parser evaluates global line/function/branch floors, function CRAP scores, critical paths, and source records. In `check --diff`, changed Git lines are filtered to AST-supported source-role files and only changed executable lines are scored. Missing, empty, unreadable, malformed, or incomplete required coverage evidence is blocking regardless of `gate.strict`.

### Mutation reports

When `[mutation].enabled` is true, `check` and `verify` evaluate Stryker-shaped, cargo-mutants-shaped, or generic outcome-count JSON. Scores use killed divided by killed plus survived. Empty reports/outcomes, missing paths, parse errors, and no viable outcomes fail. Timeout, compile-error, runner-error, and unviable outcomes are integrity findings; report evidence is always current and never ratcheted.

### Generated freshness

`[generated]` is a separate command-backed evidence engine. When enabled, a non-empty `freshness_command` runs in `check` and `verify` with its own timeout. Excluding generated files from file budgets does not disable freshness. Missing command, timeout, runner failure, or non-zero exit is current blocking evidence and is outside legacy ratchet matching.

### Legacy adoption

With `[legacy].ratchet = true`, Hardgate resolves the configured Git reference and merge base, loads a baseline snapshot, runs the static gate (plus configured dead code), and compares current findings. Existing non-worsened static debt can be grandfathered as advisories. New or worsened static debt remains blocking; retained findings are annotated with changed-file or changed-hunk context. Budget, suppression, complexity, invariant, clone, and dead-code findings participate. Coverage, mutation, generated freshness, and orchestration findings remain current blocking evidence and are not grandfathered. Missing reference, merge base, snapshot, or baseline analysis is a blocking evidence failure.

## Command boundaries

- `check`: static engines, enabled reports, freshness, and optional configured dead code.
- `check --diff`: changed/staged static scope, full-index clone matching, changed executable LCOV, and full-tree legacy static ratchet when enabled.
- `check --all`: `check` plus configured formatter/linter/test orchestration.
- `verify`: full static tree plus enabled reports, freshness, and legacy static/dead-code ratchet; no orchestration or native mutation.
- `mutate`: native unmutated baseline and AST mutants; no report ingestion.

Orchestration commands run sequentially from the repository root; a local `node_modules/.bin` is prepended to `PATH`. An unconfigured command is not inferred. A configured command that is empty, unavailable, times out, or exits non-zero yields an orchestration finding.

## Native mutation and JavaScript resolution

Native mutation is source-role only. It resolves a test command per file, runs a passing unmutated baseline, mutates one AST point at a time, classifies outcomes, and verifies byte-for-byte restoration. A failed baseline or zero viable mutants fails before a green result.

For JS/TS files, the resolver walks ancestor directories, chooses the nearest `package.json`, identifies workspace markers, and detects npm, pnpm, Yarn, or Bun from `packageManager`/lock/config markers (npm is the fallback). Jest, Vitest, and Playwright are inferred from the test script, manifest keys, or ancestor config files. The package/config root is the command working directory; the supplied repository root is the final fallback. A matching sibling or nested `<stem>.test|spec>` file is selected when reliable, otherwise the full suite runs. Manager-local `test`/`run`/`exec` commands are used, with `npm exec --offline` and `bun x --no-install` preventing runtime installation. `--test-cmd` overrides this resolver.

## Reports and MCP

The report aggregator records scan/function counts, violations, advisories, and duration, then renders terminal, agent Markdown, JSON, compact, or summary output. CLI empty discovery is an advisory while enabled evidence still runs; malformed or missing required evidence remains blocking. The MCP check surface rejects empty discovery before producing a report.

The MCP server uses newline-delimited or `Content-Length`-framed JSON-RPC over stdio. Its tools are `hardgate_check(paths?, diff?)`, `hardgate_scan_file(path)`, and `hardgate_get_metrics(path, symbol)`. `hardgate_check` routes through the static gate only, accepts optional paths and `diff`, and fails closed on invalid config/arguments, empty scopes, unreadable or unparsable files, Git failures, and empty discovery. MCP does not run coverage, mutation, freshness, dead code, orchestration, or native mutation.
