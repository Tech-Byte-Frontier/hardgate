# System architecture

Hardgate is a local Rust CLI composed of discovery, policy engines, evidence evaluators, command orchestration, and report renderers. The design keeps each source of truth close to the engine that owns it: classification chooses inputs, policy controls whether an engine is enabled, and the report records every blocking finding and advisory.

```text
CLI / MCP (stdio)
       |
       v
configuration + discovery + role classification
       |
       +--> file/anti-gaming safety
       +--> Tree-sitter complexity (supported parser targets)
       +--> declarative invariant checks
       +--> bounded token-stream clone detector
       +--> optional dead-code analysis
       +--> optional LCOV / mutation-report evidence
       +--> optional formatter/linter/test orchestration
       |
       v
aggregated report -> terminal / agent Markdown / JSON / compact / summary
```

## Discovery and roles

The walker inventories configured extensions while pruning dependency and build-output directories. Each file becomes a `ClassifiedFile` with a role and an AST-support flag:

- `source`: ordinary safety, invariants, complexity when parsed, and native mutation targets;
- `test`: safety, invariants, complexity, and clone analysis, but never a native mutation target;
- `fixture`: safety and clone analysis;
- `generated`: inventory and advisory only;
- `migration`, `config`, `documentation`, `vendor`, or `unknown`: role-specific handling described in the configuration specification.

Parser targets are Rust (`.rs`), JavaScript (`.js`, `.jsx`, `.mjs`, `.cjs`), TypeScript (`.ts`, `.tsx`, `.mts`, `.cts`), Python (`.py`), and Go (`.go`). Inventory-only formats remain visible for classification and policy but do not receive function metrics.

A configured file-budget exclusion is owned by the file-budget engine. The file is still available to classification, anti-gaming, invariants, parsing, and clone checks, and the report emits an advisory for the exclusion.

## Static engines

### Structural budgets and complexity

The Tree-sitter analyzer parses each supported source/test file and records function name, source span, parameters, statements, nesting, cyclomatic/cognitive contributors, Halstead difficulty, and ABC score. Configured ceilings produce actionable complexity findings. Parsing errors are evidence failures in strict mode rather than an empty-function success.

File budgets separately enforce raw byte and physical-line ceilings. They apply only to the extension keys configured under `[budgets.files.max_lines]` (or `default`).

### Anti-gaming

The suppression scanner reads safety-checked files line by line. It recognizes known comment/attribute directives and configured literal tokens, while avoiding common occurrences inside strings. When `disallow_suppressions` is enabled, each recognized directive is a violation. The current policy has no per-file approval path.

### Architectural invariants

Invariant rules are compiled from `from`/ `exclude` globs and optional import, call, and token patterns. The checker strips line comments and scans source lines, preserving string content where needed to avoid false positives. It does not resolve modules or type-check the project.

### Clone detection

The clone engine lexes non-comment source lines into identifiers, punctuation, normalized numbers, and normalized strings. It indexes fixed-size token windows with a rolling hash, verifies token sequences after hash matches, coalesces adjacent windows, and caps repeated-hash work. It analyzes source, test, and fixture roles unless the clone policy excludes a path.

## Evidence engines

### LCOV and CRAP

When `[coverage].enabled` is true, the coverage scorer parses LCOV records and computes global line/function/branch percentages, per-function CRAP, and optional critical-path line coverage. A missing report, malformed record, or missing source record is evidence failure under strict policy. No coverage provider is executed by Hardgate.

### Mutation reports

When `[mutation].enabled` is true, `check` and `verify` parse configured JSON reports in Stryker, cargo-mutants, or generic outcome-count shapes. Scores use killed divided by killed plus survived. Missing reports and parser errors are recorded as evidence failures; timeout handling follows `reject_timeouts`, while compile, runner, and unviable outcomes remain integrity findings.

### Native mutation run

The `mutate` command uses the AST mutation generator over source-role files. It resolves a per-file test command, runs unmutated baselines first, applies one binary/boolean mutation at a time, executes the command with a timeout, and restores the original bytes through a verification guard. A baseline failure, restoration failure, or zero viable mutants fails the command. This runner is separate from Stryker and from mutation-report ingestion.

## Orchestration and reports

`check --all` invokes configured formatter, linter, and test commands sequentially through the orchestration engine. `fmt` invokes the configured formatter (or its check command in check-only mode). A repository-local `node_modules/.bin` is prepended to child-process `PATH); command output and non-zero exits become orchestration findings.

The diagnostic aggregator freezes scan counts, duration, advisories, and violations. It renders terminal output for people, structured Markdown for agents, JSON for automation, or compact/summary views. A report passes only when its violation collections are empty.

## MCP transport

The embedded MCP server reads newline-delimited or `Content-Length`-framed JSON-RPC from standard input and writes responses to standard output. It exposes `hardgate_check`, `hardgate_scan_file`, and `hardgate_get_metrics` for static analysis. It does not run orchestration, coverage, mutation, or dead-code analysis.

## Concurrency and evidence boundaries

File reads and per-file static analysis use Rayon where safe; each engine owns its input policy and exclusions. Discovery errors, Git errors in diff mode, unreadable files, parser errors, and required-report failures are never converted into an empty success under strict policy. Disabled evidence engines do not inspect their configured stale files.

**Planned stabilization (not implemented here):** merge-base baseline/ratchet evaluation, changed-hunk coverage attribution, diff-coverage/new-clone fingerprints, and release artifact verification require separate implementation and regression proofs before they can be documented as active architecture.
