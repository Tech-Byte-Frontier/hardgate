# Existing landscape and comparative analysis

Hardgate sits at the boundary between source-level tools and a repository's acceptance policy. It can orchestrate a formatter, linter, or test command, but its own contract is narrower: deterministic structural budgets, anti-gaming checks, declarative boundaries, bounded token clones, and explicit local evidence.

## Tool positioning

| Tool | Primary role | Relationship to Hardgate |
| --- | --- | --- |
| PMAT | Rust-based agent context and technical-debt workflows, including quality scoring and MCP integration | Complementary context and grading; Hardgate supplies hard budgets, role ownership, and strict evidence semantics |
| Qlty | CLI/cloud quality workflow with maintainability metrics, coverage, duplication, and CI reporting | Complementary aggregation and trends; Hardgate remains the local policy/verdict layer |
| jscpd | Copy/paste detection using token windows and configurable duplication thresholds | A dedicated, broad-format detector; Hardgate includes a bounded token detector whose exclusions belong only to clone analysis |
| Stryker | Mutation testing for JavaScript/TypeScript and related ecosystems; executes a test runner and emits mutation reports | The mature external mutation path; Hardgate can evaluate a Stryker JSON report but does not invoke Stryker |
| SonarQube / SonarCloud | Broad static analysis, code smells, security, coverage, duplication, and hosted quality gates | Strong centralized analysis and history; Hardgate is a local, repository-owned policy with no server dependency |
| ESLint | Extensible JavaScript/TypeScript lint rules and plugins | Language-specific linting remains ESLint's job; Hardgate can run it through the orchestration lint command |
| Biome | Integrated JavaScript/TypeScript formatter and linter | Hardgate can orchestrate Biome commands; it does not embed Biome's rules |
| Oxlint | Fast JavaScript/TypeScript linting | Hardgate can orchestrate Oxlint; Oxlint remains the owner of language lint diagnostics |
| Trunk / Lefthook / pre-commit | Hooks and command orchestration | Useful scheduling layers; Hardgate provides the policy and report that a hook invokes |

No row implies feature parity. Each tool should remain responsible for the semantics it understands best.

## Comparison by concern

| Concern | Hardgate's current contract | Typical complementary tool |
| --- | --- | --- |
| Structural size and function budgets | Configurable bytes, physical lines, and Tree-sitter metrics for supported parser targets | BCA or language-specific analyzers for additional metrics |
| Suppression policy | Known directives and project tokens can be blocking findings | ESLint, Oxlint, Biome, Clippy, or type checkers for the underlying diagnostics |
| Architecture | Declarative path-scoped import/call/token rules evaluated line by line | Dependency-graph or compiler tooling for resolved relationships |
| Duplication | Bounded rolling-hash token windows over source/test/fixture roles | jscpd or Qlty when broad formats, richer reports, or hosted history are needed |
| Coverage and CRAP | Optional LCOV ingestion with global floors, CRAP, and critical paths | Vitest/Jest/cargo-llvm-cov or another provider that produces LCOV |
| Mutation | Native AST loop for selected production files, plus Stryker/cargo-mutants/generic JSON report evaluation | Stryker or cargo-mutants for language-specific mutation operators and runners |
| Commands and formatting | Optional configured formatter/linter/test commands; no implicit discovery | Biome, Oxlint, ESLint, Cargo, or a CI runner |
| Agent transport | Stdio MCP tools and structured terminal/agent/JSON output | MCP clients, hooks, and hosted dashboards |

## Why a policy layer

A project can use all of these tools and still have an ambiguous acceptance rule: tests may not have run, an excluded file may hide debt, or a report may be stale. Hardgate makes those states visible:

- each discovered file has a role before an engine receives it;
- each exclusion is owned by one engine and can produce an advisory;
- enabled evidence must be present and parseable in strict mode;
- disabled evidence is not read merely because an old report exists;
- static checks and native mutation are separate commands with separate proof obligations;
- a passing report means the configured violation collections are empty, not that every possible quality property was proven.

## Current boundaries

Hardgate currently parses six Tree-sitter targets (Rust, JavaScript, TypeScript, TSX, Python, and Go), inventories additional text/data formats, reads LCOV for coverage, and speaks MCP over stdio. It does not claim a broader parser matrix, global module resolution, another MCP transport, or a merge-base baseline/ratchet.

**Planned stabilization (not active):** reference-branch baselines, changed-hunk coverage attribution, new-clone fingerprints, consumer fixtures, and release artifact/checksum verification require implementation and regression proof before they belong in this comparison.
