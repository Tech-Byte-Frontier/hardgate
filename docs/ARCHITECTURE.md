# System Architecture: Hardgate

## 1. High-Level Overview

Hardgate is built as a modular, highly concurrent Rust application designed for near-instantaneous execution in local development, pre-commit hooks, CI/CD runners, and AI agent feedback loops.

```text
                                  ┌───────────────────────────┐
                                  │      CLI / MCP Server     │
                                  └─────────────┬─────────────┘
                                                │
                                  ┌─────────────▼─────────────┐
                                  │   Orchestrator & Runner   │
                                  │      (Rayon Threadpool)   │
                                  └─────────────┬─────────────┘
                                                │
    ┌───────────────────┬───────────────────────┼───────────────────────┬───────────────────┐
    ▼                   ▼                       ▼                       ▼                   ▼
┌───────────────┐ ┌───────────────┐   ┌───────────────────┐   ┌───────────────────┐   ┌───────────────┐
│  AST Budget   │ │  Anti-Gaming  │   │   Architectural   │   │    Clone Stream   │   │  Coverage &   │
│   Engine      │ │    Scanner    │   │     Invariants    │   │      Detector     │   │   Mutation    │
└───────┬───────┘ └───────┬───────┘   └─────────┬─────────┘   └─────────┬─────────┘   └───────┬───────┘
        │                 │                     │                       │                     │
        └─────────────────┼─────────────────────┴───────────────────────┴─────────────────────┘
                          │
                          ▼
            ┌───────────────────────────┐
            │   Diagnostic Aggregator   │
            └─────────────┬─────────────┘
                          │
              ┌───────────┴───────────┐
              ▼                       ▼
      [Terminal UI]           [Agent Protocol]
      - ANSI Color diffs      - Token-efficient JSON
      - Status tables         - Actionable LLM prompts
      - Exit code (0 or 1)    - MCP Tool Responses
```

---

## 2. The 7 Core Engines

### Engine 1: Tree-sitter AST & Complexity Engine
Instead of spawning external language compilers, Hardgate embeds `tree-sitter` and official grammars (Rust, TypeScript, JavaScript, Python, Go, C++, etc.) directly into the binary.

* **File-level metrics:**
  - Physical lines of code (excluding blank lines and comments if configured).
  - Raw byte size (enforcing limits like $\le 32$ KiB per file).
* **Function-level metrics:**
  - **Cyclomatic Complexity:** Counts branching control-flow nodes (`if`, `match`/`switch`, `for`, `while`, `&&`, `||`, ternary `? :`).
  - **Cognitive Complexity:** Measures mental nesting and flow-break penalties (based on G. Ann Campbell's Sonar specification).
  - **Halstead Metrics:** Computes vocabulary, length, volume, and Halstead difficulty $D = \frac{\eta_1}{2} \times \frac{N_2}{\eta_2}$.
  - **ABC Metric:** Aggregate of Assignments, Branches, and Conditions.
  - **Signature constraints:** Enforces maximum parameter counts ($\le 4$) and function statement counts.

### Engine 2: Anti-Gaming & Suppression Scanner
The anti-gaming engine runs across two layers:
1. **Comment Token Scanning:** Scans all comment nodes identified by Tree-sitter for known suppression directives:
   - **TypeScript / JavaScript:** `// @ts-ignore`, `// @ts-nocheck`, `/* eslint-disable */`, `oxlint-disable`, `prettier-ignore`.
   - **Rust:** `#[allow(...)]`, `#[expect(...)]`, `#![allow(...)]`, `mutants::skip`.
   - **Python:** `# type: ignore`, `# noqa`, `# pragma: no cover`.
   - **Coverage suppressions:** `c8 ignore`, `istanbul ignore`, `coverage(off)`.
2. **Policy Enforcement:** By default, *any* suppression directive triggers an immediate failure. If exemptions are permitted, they must reference a signed human override file rather than an inline agent edit.

### Engine 3: Architectural Invariant Linter
This engine statically inspects imports, exports, and call hierarchies to prevent architectural boundary degradation:
* Constructs an in-memory directed dependency graph of file imports using AST import declaration queries.
* Evaluates rules specified in `hardgate.toml`:
  ```toml
  [[invariants.rules]]
  from = "src/components/**"
  disallow_imports = ["@tauri-apps/api*", "src/server/**", "src/db/**"]
  message = "UI components must route calls through domain services."
  ```
* Flags unauthorized calls (e.g., direct `fetch` invocations in components).

### Engine 4: High-Performance Clone Detector
Rather than relying on heavy external utilities, Hardgate implements a streaming **Rabin-Karp / Winnowing token hashing algorithm**:
* AST tokenization normalizes identifiers and literal values while preserving structural syntax tokens.
* A rolling hash over a sliding token window (e.g., 50 tokens) flags identical or near-identical subtrees across files.
* Produces zero-duplicate guarantees across production and test code.

### Engine 5: Coverage & CRAP Scorer
* Parses standard coverage artifacts:
  - `lcov.info` (LCOV format)
  - `cobertura.xml`
  - JSON summaries (e.g., from `cargo-llvm-cov` or Vitest)
* Computes the **CRAP (Change Risk Anti-Patterns)** metric per function:
  $$\text{CRAP}(m) = \text{comp}(m)^2 \cdot (1 - \text{cov}(m))^3 + \text{comp}(m)$$
  Where $\text{comp}(m)$ is cyclomatic complexity and $\text{cov}(m) \in [0, 1]$ is executable line coverage.
* Functions with high complexity and low coverage fail the gate if $\text{CRAP} \ge 25$.

### Engine 6: Mutation Gatekeeper
* Interfaces with mutation runners:
  - Rust: `cargo-mutants`
  - JavaScript / TypeScript: Stryker Mutator
  - Python: `mutmut`
* Validates baseline integrity: rejects runs with timeouts, missing test files, or unviable mutants.
* Enforces hard kill-rate thresholds (e.g., minimum 85% mutant detection).

### Engine 7: Agent Diagnostic Protocol & Native MCP Server
Hardgate natively supports two operational modes for AI agents:
1. **Token-Efficient CLI Output (`--format agent`):**
   Outputs compact, structured markdown optimized for LLM attention spans:
   ```markdown
   ❌ Hardgate Violation in `src/payment.ts:42`
   - Function: `processTransaction`
   - Violation: Cognitive complexity is 18 (budget: 15)
   - Recommendation: Extract branch at line 58 into helper `validateCardToken`.
   ```
2. **Model Context Protocol (MCP) Server:**
   Runs as an MCP server (`hardgate mcp`), registering tools with the agent:
   - `hardgate_verify`: Runs the gate on the current branch.
   - `hardgate_check_file`: Runs AST metrics and anti-gaming checks on a single file before the agent writes changes.
   - `hardgate_metrics`: Retrieves cyclomatic and cognitive metrics for any function symbol.

---

## 3. Execution Pipeline & Concurrency

Hardgate uses [Rayon](https://github.com/rayon-rs/rayon) for parallel work distribution:

1. **Discovery Phase:** Scans git-tracked files using `ignore` / `git ls-files` ($< 5\text{ms}$).
2. **Parallel Parsing & Analysis Phase:**
   Files are partitioned across CPU cores. Each thread runs Tree-sitter parsers, computes complexity metrics, scans for suppressions, and extracts import tokens in parallel ($< 30\text{ms}$ for 1,000 files).
3. **Graph & Clone Phase:**
   Import graph validation and token window hash lookups are computed in a single unified sweep.
4. **Report & Verdict Phase:**
   Results are merged into an immutable verdict object. If any hard budget or anti-gaming rule is violated, Hardgate exits with non-zero status and prints the diagnostic report.
