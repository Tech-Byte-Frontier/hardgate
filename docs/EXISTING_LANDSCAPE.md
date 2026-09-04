# Existing landscape and comparative analysis

Hardgate sits between language tools and a repository's acceptance policy. It can invoke a formatter, linter, or test command, but its own contract is narrower: deterministic structural budgets, role-aware inputs, anti-gaming checks, declarative boundaries, bounded clone detection, and explicit evidence requirements.

## Tool positioning

| Tool | Owns | Relationship to Hardgate |
| --- | --- | --- |
| PMAT | Rust-oriented agent context and technical-debt workflows, including quality scoring and MCP integration | Complementary context and grading; Hardgate supplies repository-owned budgets, role policy, and fail-closed evidence |
| [Qlty CLI / Qlty Cloud](https://docs.qlty.sh/what-is-qlty) | A local CLI for setup, linters/formatters, smells and metrics, plus coverage publishing to a hosted code-health platform with maintainability, duplication, lint, and coverage views | Complementary analysis and history; Hardgate is the local verdict layer with explicit evidence, role ownership, and legacy static ratchet. Qlty's AST duplication analysis and Hardgate's normalized-token clone fingerprints answer related but different questions |
| jscpd | Dedicated copy/paste detection across broad formats | A specialized detector; Hardgate includes a bounded role-group token detector and keeps clone exclusions local to that engine |
| Stryker | JavaScript/TypeScript mutation execution and report generation | A mature external mutation runner; Hardgate evaluates Stryker-shaped JSON but does not invoke Stryker |
| SonarQube / SonarCloud | Broad static analysis, code smells, security, coverage, duplication, and hosted quality gates | Centralized analysis and history; Hardgate is local and repository-owned, and its verdict does not depend on a hosted service |
| ESLint | Extensible JavaScript/TypeScript lint rules and plugins | Language-specific linting remains ESLint's job; Hardgate can run it through `[orchestration].lint` |
| Biome | JavaScript/TypeScript formatting and linting | Hardgate can orchestrate Biome commands; it does not embed Biome rules |
| Oxlint | JavaScript/TypeScript linting | Hardgate can orchestrate Oxlint; Oxlint owns language diagnostics |
| Trunk / Lefthook / pre-commit | Hook scheduling and command orchestration | Useful invocation layers; Hardgate supplies the policy report they invoke |

Qlty should not be described as a drop-in Hardgate implementation. Qlty Cloud is a hosted code-health product, and the Qlty CLI can run local analysis and publish coverage. Hardgate does not upload data or provide a hosted dashboard; it decides a local gate from the repository's configured engines and evidence. Use both when a project wants local acceptance plus hosted trends.

## Comparison by concern

| Concern | Hardgate contract | Complementary tool or provider |
| --- | --- | --- |
| Structural size and function budgets | Configurable bytes, physical lines, and Tree-sitter metrics for supported parser targets | Language analyzers for additional metrics |
| Suppression policy | Known directives and project tokens can be blocking findings | ESLint, Oxlint, Biome, Clippy, type checkers, and coverage tools for underlying diagnostics |
| Architecture | Declarative path-scoped import/call/token rules evaluated line by line | Dependency-graph or compiler tooling for resolved relationships |
| Duplication | Bounded normalized-token windows over independent source/test/fixture role groups; stable path/line-independent fingerprints | Qlty or jscpd for broader formats, different structural algorithms, or hosted history |
| Coverage and CRAP | LCOV ingestion with global floors, CRAP, critical paths, and changed executable-line mode | Jest/Vitest/cargo-llvm-cov or another provider that emits LCOV; Qlty Cloud for publication/history |
| Mutation | Native AST baseline + mutants, plus Stryker/cargo-mutants/generic JSON report evaluation | Stryker or cargo-mutants for language-specific operators and runners |
| Generated artifacts | Independent freshness command; budget/clone exclusions cannot disable it | Project generator and CI command that establishes the freshness check |
| Existing-code adoption | Git reference/merge-base static ratchet with rename lineage and changed-hunk attribution | Hosted quality baselines or migration tooling with different debt models |
| Commands and formatting | Optional configured formatter/linter/test commands; no implicit discovery | Biome, Oxlint, ESLint, Cargo, or CI runner |
| Agent transport | Stdio MCP tools and terminal/agent/JSON output | MCP clients, hooks, and hosted dashboards |

## Why a policy layer

A project can use all of these tools and still have an ambiguous acceptance rule: tests may not have run, an excluded file may hide debt, or a report may be stale. Hardgate makes those states explicit:

- every discovered file has a role before an engine receives it;
- every exclusion is owned by one engine and can emit an advisory;
- enabled evidence must be present, non-empty, and parseable;
- disabled evidence is not read merely because an old report exists;
- static, report evaluation, orchestration, and native mutation are separate commands;
- legacy adoption ratchets static/dead-code debt only, while coverage, mutation, freshness, and orchestration remain current blocking evidence;
- a passing report means its configured violation collections are empty, not that every possible quality property was proven.

## Current boundaries

Hardgate parses Rust, JavaScript, TypeScript/TSX, Python, and Go with Tree-sitter; inventories additional text/data formats; reads LCOV; evaluates several mutation JSON shapes; and speaks MCP over stdio. It does not claim global module resolution, compiler/type analysis, a broader parser matrix, another MCP transport, or a hosted quality dashboard.

The npm wrapper and shell installer publish/select six Unix artifacts (Linux x64/arm64 glibc and musl, macOS x64/arm64). Release archives are checked with `SHA256SUMS`, build metadata, and exact version/commit identity. There is no Windows or Homebrew installer in this contract.
