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
2. each engine owns its exclusions and emits an advisory when it excludes a file;
3. enabled evidence is required, and empty or missing inputs fail closed;
4. disabled evidence is not read merely because an old report remains on disk;
5. static checks, orchestration, report evaluation, and native mutation have separate commands and proof obligations.

The result is not a universal quality proof. It is a truthful statement about the configured policy and evidence that this run actually evaluated.

## The local policy model

### Roles before rules

Source, test, generated, fixture, and migration are first-class roles with independent severity, size/complexity budgets, and clone thresholds. Native mutation is source-role-only, with source eligibility configurable; other roles remain ineligible. Configuration/documentation/vendor/unknown roles have narrower built-in handling. Ordered custom classification rules let a repository state its own conventions, while dependency/build pruning remains authoritative.

Generated artifacts illustrate the boundary: they can be inventoried and excluded from handwritten debt checks, but `[generated].freshness_command` is a separate command-backed check. Excluding a generated path from file budgets never disables freshness.

### Structural budgets

Physical byte/line ceilings make growth visible at the file boundary. Tree-sitter metrics provide cyclomatic and cognitive complexity, Halstead difficulty, ABC score, parameter count, statement count, function lines, and nesting depth for Rust, JavaScript, TypeScript/TSX, Python, and Go. Presets scale those values; explicit TOML keys override one value without requiring a copied preset.

### Anti-gaming and architecture

The anti-gaming scanner recognizes common compiler, linter, type-checker, and coverage suppression directives plus project-forbidden tokens in safety-checked roles. There is no inline approval channel. Declarative invariant rules inspect imports, calls, and tokens on configured paths. They complement a compiler or dependency graph tool; they do not resolve modules or type-check a project.

### Evidence as an input contract

Coverage and mutation report policies are optional only when disabled explicitly. When enabled, coverage requires a non-empty, parseable LCOV report; mutation requires a non-empty, recognized JSON report with outcomes. Missing source records, empty reports, malformed records, and integrity outcomes are blocking regardless of `gate.strict`. A disabled policy ignores stale files.

`check` evaluates static engines plus enabled reports and generated freshness. `check --diff` scopes ordinary static findings to changed/staged files, uses a full clone index, and evaluates changed executable LCOV lines; with a legacy ratchet, static and clone analysis uses the full current selected scope (whole tree when no path filters are supplied) while LCOV remains diff-scoped. `check --all` adds only configured formatter/linter/test commands. `verify` runs full static and configured evidence by default; optional path filters scope only static inventory and coverage source matching, while mutation reports, freshness, and legacy ratchet evidence remain configured/full. It does not run orchestration or native mutation. When mutation is enabled, `mutate` runs a native unmutated baseline and AST mutants; when disabled, it prints a disabled-policy note and exits successfully without target discovery or execution. Any `--diff` mutation invocation, including one with `--scoped`, is an explicit no-op when no changed production target exists; only a non-diff unrestricted or scoped enabled run with no eligible target fails.

### Legacy adoption without a freeze

Existing repositories need a path to stricter policy without hiding new debt. A configured legacy reference resolves a Git merge base and compares baseline static findings (plus configured dead code) with the current report. Non-worsened findings can be grandfathered as advisories; new or worsened findings with effective role severity `error` stay blocking, `warning` findings remain advisories, and `ignore` findings are omitted. Changed-file and changed-hunk attribution shows why a retained finding is relevant. Rename lineage and clone fingerprints are path/line independent where identity is safe. Coverage, mutation, generated freshness, and orchestration remain current blocking evidence and are never grandfathered.

### Native mutation feedback

`hardgate mutate` is an executable feedback loop, not a score copied from a report. When enabled, it selects source-role files, resolves an appropriate test command, runs the unmutated baseline first, applies bounded binary/boolean mutations, classifies killed/survived/timeout/compile/runner/equivalent/unviable outcomes, and restores source bytes after each attempt. A failed baseline or no viable mutation point is a failure; when disabled, the command reports a successful no-op.

For JavaScript and TypeScript, resolution walks from the source toward the repository root, validates encountered manifests, recognizes only declared workspaces (lockfiles are manager hints), detects npm/pnpm/Yarn/Bun, and infers Jest/Vitest/Playwright only when selector behavior is unambiguous. A child test script wins; one `test:*` script is allowed, and a reliable child-local framework package or config signal wins over a validated enclosing workspace-root script. The root script is a fallback only when the child has no local script or reliable local signal; malformed manifests and ambiguous scripts fail closed. It searches for a matching test file and falls back to the full suite. Local manager commands are used without runtime installation; `--test-cmd` is the authoritative escape hatch for project-specific runners.

## Presets are explicit policy bundles

- `strict-agent`: tight structural limits and enabled configured coverage/mutation evidence.
- `balanced`: scaled structural limits with coverage/mutation report engines disabled.
- `legacy-migration`: scaled structural limits, coverage/mutation report engines disabled, and static reference/merge-base ratchet enabled.
- `custom`: only explicit values plus deserialized defaults.

The no-config `strict-agent` object is exactly what `hardgate init --preset strict-agent` renders. Presence-based merging means an omitted field inherits the preset; an explicit `false` or empty value remains a deliberate override.

## Reviewable output and agent transport

The same report can be rendered for terminal readers, agent context, or automation. JSON is a single structured report; agent Markdown includes locations, actual values, limits, and recommendations. Advisories keep exclusions, grandfathered debt, and partial command scope visible without turning them into pass criteria.

The MCP server is stdio-only and static-only for `hardgate_check(paths?, diff?)`, `hardgate_scan_file(path)`, and `hardgate_get_metrics(path, symbol)`. The check tool uses the CLI static path and fails closed on invalid configuration/arguments, empty scopes, missing or unreadable files, parser/Git errors, and empty discovery. It does not run reports, freshness, dead code, orchestration, or native mutation.

## What Hardgate is (and is not)

Hardgate is a deterministic, repository-owned policy and reporting layer for agent-assisted work. It complements compilers, language linters, formatters, coverage providers, mutation runners, clone tools, and hosted quality dashboards. It does not replace their language-specific semantics, infer a test command that was not configured, or claim more evidence than its report contains.
