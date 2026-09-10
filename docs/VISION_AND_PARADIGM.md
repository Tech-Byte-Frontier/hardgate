# Vision and paradigm: deterministic policy for agent-assisted code

Autonomous coding agents make plausible code inexpensive. The scarce resource is review time: a maintainer needs to know which files were inspected, which policy applied, and whether the evidence is current. Hardgate treats acceptance as a local policy problem rather than a prompt-writing problem.

```text
probabilistic agent -> deterministic local policy -> actionable report
                         | roles and budgets
                         | anti-gaming and invariants
                         | evidence and freshness
                         | explicit command boundaries
```

## What a green command must mean

An agent can optimize for an exit code by adding suppression directives, moving code into an unexamined role, copying a nearby implementation, or pointing at a stale report. Hardgate makes those choices visible:

1. inventory files receive a role before engines select inputs;
2. file-budget and clone exclusions belong to their owning engines and emit advisories;
3. enabled evidence is required, and empty or missing inputs fail closed;
4. disabled evidence is not read merely because an old report remains on disk;
5. static checks, orchestration and report evaluation have distinct proof obligations.

The result is not a universal quality proof. It is a truthful statement about the configured policy and evidence that this run actually evaluated.

## The local policy model

### Roles before rules

Source, test, generated, fixture, and migration are first-class roles with independent severity, size/complexity budgets, and clone thresholds. Native mutation is source-role-only, with source eligibility configurable; other roles remain ineligible. Configuration/documentation/vendor/unknown roles have narrower built-in handling. Ordered custom classification rules let a repository state its own conventions, while dependency/build pruning remains authoritative.

Generated artifacts illustrate the boundary: they can be inventoried and excluded from handwritten debt checks, but `[generated].freshness_command` is a separate command-backed check. Excluding a generated path from file budgets never disables freshness.

### Structural budgets

Physical byte/line ceilings make growth visible at the file boundary. Tree-sitter metrics provide cyclomatic complexity, parameter count, statement count, function lines, and nesting depth for Rust, Python, JavaScript, TypeScript/TSX. Presets scale those values; explicit TOML keys override one value without requiring a copied preset.

### Anti-gaming and architecture

The anti-gaming scanner recognizes common compiler, linter, type-checker, and coverage suppression directives plus project-forbidden tokens in safety-checked roles. There is no inline approval channel. Declarative invariant rules inspect imports, calls, and tokens on configured paths. They complement a compiler or dependency graph tool; they do not resolve modules or type-check a project.

### Evidence as an input contract

In `strict-agent` (including no-config execution), coverage and mutation report policies are required unless explicitly disabled. `balanced` and `legacy-migration` disable them, while `custom` uses deserialized defaults. Whenever enabled, coverage requires a non-empty, parseable LCOV report and mutation requires a non-empty, recognized JSON report with outcomes. Missing source records, empty reports, malformed records, and integrity outcomes are blocking regardless of `gate.strict`. A disabled policy ignores stale files.

`check` evaluates static engines plus enabled reports and generated freshness. `check --diff` selects Git-changed/staged inventory by default, adds explicit existing paths to static/clone selection, and evaluates LCOV only on the intersection with actual changed executable lines; with a legacy ratchet, static and clone analysis uses the full current selected scope (whole tree when no paths are supplied) while that changed-line LCOV intersection remains. `check` also verifies formatting and linting by default, runs configured tests/type checks, and requires enabled evidence. `--checks` selects explicit partial groups; the report separates their success from complete acceptance. Source-bound specialist receipts replace unverified report reuse.

### Legacy adoption without a freeze

Existing repositories need a path to stricter policy without hiding new debt. A configured legacy reference resolves a Git merge base and compares baseline static findings with the current report. Non-worsened findings can be grandfathered as advisories; new or worsened findings with effective role severity `error` stay blocking, `warning` findings remain advisories, and `ignore` findings are omitted. Changed-file and changed-hunk attribution shows why a retained finding is relevant. Rename lineage and clone fingerprints are path/line independent where identity is safe. Coverage, mutation, generated freshness, and orchestration remain current blocking evidence and are never grandfathered.

### Specialist mutation evidence

Mutation execution belongs to cargo-mutants and Stryker. Hardgate retains
report validation and configured evidence requirements, with no built-in
mutant generator or project test-runner inference.

## Presets are explicit policy bundles

- `strict-agent`: tight structural limits and enabled configured coverage/mutation evidence.
- `balanced`: scaled structural limits with coverage/mutation report engines disabled.
- `legacy-migration`: scaled structural limits, coverage/mutation report engines disabled, and static reference/merge-base ratchet enabled.
- `custom`: only explicit values plus deserialized defaults.

The no-config `strict-agent` object is exactly what `hardgate init --preset strict-agent` renders. Presence-based merging means an omitted field inherits the preset; an explicit `false` or empty value remains a deliberate override.

## Reviewable output and agent transport

The same report can be rendered for terminal readers, agent context, or automation. JSON is a single structured report; agent Markdown includes locations, actual values, limits, and recommendations. Advisories keep exclusions, grandfathered debt, and partial command scope visible without turning them into pass criteria.

The MCP server is stdio-only and static-only for `hardgate_check(paths?, diff?)`, `hardgate_scan_file(path)`, and `hardgate_get_metrics(path, symbol)`. The check tool uses the CLI static path; `diff` selects Git-changed/staged inventory by default, explicit existing paths add to static/clone selection, and clone matching uses the full repository index. MCP never runs coverage or other reports, freshness, orchestration. Invalid arguments/configuration, missing paths, empty scopes/discovery, and Git failures are outer tool errors. Read/parse failures remain report-level Hardgate `Failed` findings whose effective role severity makes them errors, advisories, or omitted findings (`error`, `warning`, or `ignore`). For `hardgate_scan_file`, a read failure is an outer tool error while parse/static findings remain in its per-file report; `hardgate_get_metrics` reports read or missing-symbol errors explicitly.

## What Hardgate is (and is not)

Hardgate is a deterministic, repository-owned policy and reporting layer for agent-assisted work. It complements compilers, language linters, formatters, coverage providers, mutation runners, clone tools, and hosted quality dashboards. It does not replace their language-specific semantics, infer a test command that was not configured, or claim more evidence than its report contains.

## 0.6.0 scope freeze

The core is Rust/JavaScript/TypeScript structural budgets, duplication, source
roles, explicit boundary rules, suppression and policy-change detection, and
trustworthy check execution and evidence. New analyzers are deferred. Specialist
tools retain responsibility for deeper language and framework analysis; their
execution and structured findings remain distinct from Hardgate measurements.
