# Hardgate

**Deterministic quality gates, hard budgets, and anti-gaming verification harness for the AI agent era.**

[![Crates.io](https://img.shields.io/crates/v/hardgate.svg)](https://crates.io/crates/hardgate)
[![Documentation](https://docs.rs/hardgate/badge.svg)](https://docs.rs/hardgate)
[![License](https://img.shields.io/crates/l/hardgate.svg)](https://github.com/Tech-Byte-Frontier/hardgate#license)
[![Diff Speed](https://img.shields.io/badge/git_diff-%3C10ms-brightgreen.svg)](#benchmarks)
[![Downloads](https://img.shields.io/crates/d/hardgate.svg)](https://crates.io/crates/hardgate)

---

<p align="center">
  <img src="https://raw.githubusercontent.com/Tech-Byte-Frontier/hardgate/main/media/x_article_cover_clean.png" alt="Hardgate Header" width="100%" />
</p>

## The Problem: Goodhart's Law in the Agent Era

In the era of autonomous coding assistants (Claude Code, Cursor, Codex, Devin, Cline, Aider), code generation is essentially free. However, LLM agents are token-efficient cost minimizers operating under **Goodhart’s Law**:

> *"When a measure becomes a target, it ceases to be a good measure."*

When instructed to *"make the tests pass"* or *"fix the compiler error"*, unconstrained agents optimize for the fastest path to exit code `0`:
* **Suppression Pragmas:** Quietly slipping `// @ts-ignore`, `/* eslint-disable */`, `#[allow(...)]`, `# noqa`, or `/* c8 ignore */` above signatures instead of fixing underlying types.
* **Complexity Dumping:** Wrapping complex edge cases in 7-level nested conditionals and 200-line monolithic functions rather than designing clean interfaces.
* **Vacuous Coverage:** Writing assertion-free tests (`expect(true).toBe(true)`) or mocking everything to artificially hit 100% line coverage with 0% semantic verification.
* **Boilerplate Duplication:** Copy-pasting 40 lines of code across files because local duplication is cheaper in tokens than discovering and refactoring shared abstractions.
* **Architectural Leakage:** Bypassing boundaries (e.g. calling native OS APIs or raw database queries directly from UI button components).

**You cannot prompt-engineer your way out of software entropy.** Pleading with LLMs through 10-page `.cursorrules` or `CLAUDE.md` files is subjective, non-deterministic, and burns context window tokens on every single turn.

**Hardgate** is a single, zero-dependency binary written in Rust that provides machine-enforced deterministic physics that AI agents **physically cannot game**.

---

## Proven in Production: Real-World Benchmarks

### 1. Large Fullstack Monorepo (Next.js + Node.js + Shared Packages)
Dropped cold into an enterprise repository that had never run Hardgate before:
* **Scanned:** **517 files** and **4,230 functions** using Tree-sitter ASTs.
* **Execution Time:** **482 milliseconds**.
* **Findings:** Uncovered **89 duplicate code clones** and **15 hidden suppression pragmas** before traditional linters could even finish parsing config files.

### 2. Production Desktop Application (Rust + React/TypeScript)
Migrated a production desktop app (172 source files, 1,954 functions):
* **AST Scan Speed:** Evaluated in **165 ms**; incremental `hardgate check --diff` completed in **7 ms**.
* **Bloat Eliminated:** Replaced 55MB+ of local binaries (`bca`, `jscpd`, `knip`) and erased **~1,000 lines of fragile Node.js glue scripts**.

---

## Comparison: Hardgate vs. Legacy Toolchains

| Feature | Legacy Toolchain (ESLint + Knip + jscpd + Stryker) | Hardgate Quality Harness |
| :--- | :--- | :--- |
| **Execution Speed** | 30,000ms – 60,000ms | **7ms – 165ms** |
| **Footprint** | 150MB+ of npm dependencies & Node glue scripts | **Single ~15MB zero-dependency Rust binary** |
| **Multi-Language AST** | Fragmented per-language runtimes | **Unified Tree-sitter AST: Rust, TS/TSX, JS, Python, Go** |
| **Anti-Gaming** | Trivially bypassed via `@ts-ignore` / `#[allow(...)]` | **Zero-tolerance suppression detection** |
| **Clone Detection** | Slow regex/token scrapers (`jscpd`) | **Rabin-Karp token-stream rolling hash** |
| **Agent Integration** | Unstructured terminal text dumps | **Native MCP server & structured `--format agent` Markdown** |
| **Mutation Testing** | Heavy, fragile external runners with frequent timeouts | **Native Tree-sitter AST mutator with RAII rollbacks** |
| **Technical Debt** | Invisible bypasses in config files | **Prominent advisory warnings for excluded paths** |

---

## Core Engines & Capabilities

### 1. Hard Physical & AST Complexity Budgets
Computes Cyclomatic, Cognitive (Sonar model), Halstead difficulty, and ABC (Assignments, Branches, Conditions) metrics per function using Tree-sitter. Enforces strict parameter limits ($\le 4$), nesting depths ($\le 4$), and physical line thresholds.

### 2. Zero-Tolerance Anti-Gaming Engine
Scans ASTs and comment tokens across all languages for compiler, linter, type-checker, and coverage suppression pragmas. Any attempt to silence errors fails the gate immediately.

### 3. Native AST Mutation Testing (`hardgate mutate`)
Validates that test suites actually catch bugs. Automatically targets binary operators (`==`, `!=`, `<`, `>`, `&&`, `||`, `+`, `-`) and boolean literals, executes scoped tests with strict per-mutant timeouts, and guarantees instant disk restoration via RAII `RollbackGuard`.

### 4. High-Performance Clone Detection
Token-stream rolling hash (Rabin-Karp / Winnowing) catching cross-file duplicates ($\ge 5$ lines, $\ge 50$ tokens) before agents merge copy-paste redundancy.

### 5. Architectural Invariants
Declarative boundary enforcement preventing forbidden imports or unauthorized calls between subsystems (e.g., UI $\to$ native IPC or database drivers).

### 6. Dead Code & Ghost Module Extermination (`hardgate check --dead-code`)
Detects orphaned modules and unreferenced exports left behind when agents switch implementation strategies halfway through.

### 7. Technical Debt Advisory Notices
When files are bypassed in `[clones].excludes` or `[budgets.files.exclusions]`, Hardgate emits high-visibility advisory notices during checks:
```text
⚠️  Advisory: 25 files excluded from clone detection via hardgate.toml.
```
This ensures technical debt remains visible and accounted for over time.

### 8. Tool Orchestration (`hardgate fmt` & `hardgate check --all`)
Automatically detects local `./node_modules/.bin` and global paths, running formatters and linters (`oxfmt`, `cargo fmt`, `oxlint`, `clippy`) in a single unified command.

---

## Quickstart

### Installation

```sh
# Via npm (recommended for JS/TS projects, no Rust toolchain needed)
npm i -D @tech-byte-frontier/hardgate
npx hardgate check

# pnpm / Yarn / Bun (same pattern: devDependency + exec)
pnpm add -D @tech-byte-frontier/hardgate
pnpm exec hardgate check

# Via shell (Linux/macOS), no npm needed
curl -fsSL https://raw.githubusercontent.com/Tech-Byte-Frontier/hardgate/main/scripts/install.sh | sh

# Via Cargo
cargo install hardgate

# Or clone and build from source
git clone https://github.com/Tech-Byte-Frontier/hardgate.git
cd hardgate
cargo install --path . --locked
```

### Initialize in Any Repository

```sh
# Autonomous AI Agent pair-programming preset (default):
hardgate init --preset strict-agent

# Balanced preset for human-AI hybrid teams:
hardgate init --preset balanced

# Migration preset for existing codebases burning down tech debt:
hardgate init --preset legacy-migration
```

### Common Commands

```sh
# Standard check across all source files (<200ms)
hardgate check

# Check only git-modified or staged files (sub-10ms, perfect for pre-commit)
hardgate check --diff

# Full verification: static gates + dead code + orchestration (format + lint)
hardgate check --all --dead-code

# Scoped 1ms AST metric inspection on a single file
hardgate scan src/services/auth.ts

# Format code directly using configured orchestration tools
hardgate fmt

# Run native Tree-sitter AST mutation testing
hardgate mutate --diff
```

---

## AI Agent Integration (MCP & Claude Code)

### Model Context Protocol (MCP) Setup
Register Hardgate as a native tool server in your assistant's settings (Claude Desktop, Cursor, Windsurf, Cline):

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

This exposes three deterministic tools:
1. `hardgate_check(paths?: string[])`: Runs quality gates on specified files or the entire repository.
2. `hardgate_scan_file(path: string)`: Inspects complexity and suppressions for a specific file in 1ms.
3. `hardgate_get_metrics(symbol: string, path: string)`: Returns cyclomatic, cognitive, and line metrics for any function.

### Claude Code / Cursor Rule Integration

Add to your `CLAUDE.md` or `.cursorrules`:
```markdown
Before reporting completion, always verify your edits:
`hardgate check --diff --format agent`

If Hardgate reports violations, refactor immediately. Never insert suppression pragmas.
```

When called with `--format agent`, Hardgate delivers structured, pinpoint AST breakdowns directly into the LLM context window:

```markdown
❌ **Hardgate Failed**: 1 violations detected across 1 files.

### ⚡ Complexity in `packages/backend/src/lib/match-service.ts:36`
- Function: `matchCategory`
- Metric: Cyclomatic Complexity is 11 (Budget limit: 10)
- Key AST Contributors:
  - Line 40: +1 for conditional branch (`if`)
  - Line 43: +1 for conditional branch (`if`)
  - Line 46: +1 for loop (`for`)
- Actionable Refactor: Refactor `matchCategory`: extract decision branches into helper functions.
```

---

## Configuration (`hardgate.toml`)

```toml
[gate]
name = "my-service"
preset = "strict-agent"
strict = true

[budgets.files]
max_bytes = 32768

[budgets.files.max_lines]
rs = 499
ts = 400
tsx = 400
js = 400
py = 400
default = 350

# Excluded paths trigger advisory warnings to keep technical debt visible
[budgets.files.exclusions]
paths = [
  "src/generated/**"
]

[budgets.functions]
max_cyclomatic = 10
max_cognitive = 15
max_halstead_difficulty = 80.0
max_parameters = 4
max_lines = 80
max_nesting_depth = 4

[anti_gaming]
disallow_suppressions = true

[clones]
enabled = true
min_lines = 5
min_tokens = 50
excludes = [
  "**/tests/**"
]

[mutation]
enabled = true
min_score = 85.0
timeout_secs = 10
max_mutants = 30

[orchestration]
format_check = "oxfmt --check ."
format = "oxfmt ."
lint = "oxlint --type-aware ."

[analysis.dead_code]
enabled = true
entry_points = [
  "src/main.rs",
  "src/lib.rs",
  "src/index.ts"
]
```

---

## Documentation

* [Vision & Paradigm: Harness Engineering](https://github.com/Tech-Byte-Frontier/hardgate/blob/main/docs/VISION_AND_PARADIGM.md)
* [Configuration Specification (`hardgate.toml`)](https://github.com/Tech-Byte-Frontier/hardgate/blob/main/docs/CONFIGURATION_SPEC.md)
* [CLI Reference & Agent Integration](https://github.com/Tech-Byte-Frontier/hardgate/blob/main/docs/CLI_AND_INTEGRATION.md)
* [Existing Landscape & Comparative Analysis](https://github.com/Tech-Byte-Frontier/hardgate/blob/main/docs/EXISTING_LANDSCAPE.md)
* [System Architecture](https://github.com/Tech-Byte-Frontier/hardgate/blob/main/docs/ARCHITECTURE.md)
* [API Reference on docs.rs](https://docs.rs/hardgate)

---

## License

Dual-licensed under either:
* Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/Tech-Byte-Frontier/hardgate/blob/main/LICENSE-APACHE))
* MIT License ([LICENSE-MIT](https://github.com/Tech-Byte-Frontier/hardgate/blob/main/LICENSE-MIT))

at your option.
