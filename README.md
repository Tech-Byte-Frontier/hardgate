# Hardgate

**Deterministic quality gates, hard budgets, and anti-gaming verification for the AI agent era.**

[![Rust](https://img.shields.io/badge/rust-stable-brightgreen.svg)](https://www.rust-lang.org/)
[![License: MIT/Apache-2.0](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![crates.io](https://img.shields.io/badge/crates.io-hardgate-orange.svg)](https://crates.io/)

---

## The Problem: Fluency Without Correctness

In the era of autonomous AI coding agents (Claude Code, Cursor, Devin, Cline, Antigravity, Aider), code generation is essentially free. However, LLMs are probabilistic optimizers operating under **Goodhart’s Law**:

> *"When a measure becomes a target, it ceases to be a good measure."*

When instructed to *"make the tests pass"* or *"fix the bug"*, unconstrained agents routinely game conventional metrics:
- **Suppression Pragmas:** Inserting `// @ts-ignore`, `eslint-disable`, `#[allow(warnings)]`, `# noqa`, or `/* c8 ignore */` instead of fixing root problems.
- **Complexity Dumping:** Wrapping complex edge cases in 5-level nested `if/else` ladders and 150-line functions rather than refactoring.
- **Vacuous Coverage:** Writing assertion-free test executions or excessive mocks that artificially inflate line coverage to 100%.
- **Clone Proliferation:** Duplicating 30 lines of code across files because local context is cheaper than discovering existing helpers.
- **Architectural Leakage:** Bypassing abstractions (e.g., calling platform native APIs or databases directly from UI components).

**Hardgate** is a single, zero-dependency, blazingly fast Rust CLI and [Model Context Protocol (MCP)](https://modelcontextprotocol.io) server that transforms any repository into an impenetrable, deterministic quality gate that agents cannot bypass.

---

## Core Pillars

1. **Hard Physical & AST Budgets**
   Enforce non-negotiable physical line counts, byte sizes, cyclomatic ($\le 10$), cognitive ($\le 15$), Halstead difficulty ($< 80$), parameter count ($\le 4$), and nesting depth ($\le 4$) in microseconds using Tree-sitter.

2. **Zero-Tolerance Anti-Gaming Engine**
   AST-aware scanning for compiler, linter, type-checker, and coverage suppression directives across 20+ programming languages. Any attempt by an agent to silence a diagnostic fails the build.

3. **Architectural Invariant Linter**
   Declarative boundary enforcement preventing forbidden imports, dependency leaks, or unauthorized calls between subsystems (e.g., UI $\to$ native IPC).

4. **High-Performance Clone Detection**
   Token-stream rolling hash (Rabin-Karp / Winnowing) catching cross-file duplicates ($\ge 5$ lines, $\ge 50$ tokens) before agents merge redundant logic.

5. **CRAP Score & Mutation Testing Floors**
   Per-function Change Risk Anti-Patterns (CRAP $< 25$) combining executable branch/line coverage with complexity. Strict $\ge 85\%$ mutation score enforcement (Stryker, `cargo-mutants`).

6. **Agent-First Output & Native MCP Server**
   Actionable, token-efficient diagnostics with exact line numbers and prescriptive refactoring guidance for LLM context windows, plus an embedded MCP server exposing `verify()` and `inspect()` tools.

---

## Quickstart

### Installation

```sh
# Via Cargo
cargo install hardgate

# Or homebrew / direct binary release
curl -sSf https://raw.githubusercontent.com/Tech-Byte-Frontier/hardgate/main/install.sh | sh
```

### Initialize in Any Project

```sh
cd /path/to/any-repo
hardgate init --preset strict-agent
```

This generates `hardgate.toml`. Run the gate locally or in your CI:

```sh
# Run fast deterministic gate (AST budgets, anti-gaming, invariants, clones)
hardgate check

# Run full gate with coverage and mutation verification
hardgate verify

# Launch as MCP server for Claude Code / Cline / Cursor
hardgate mcp
```

---

## Documentation

- [Vision & Paradigm: Harness Engineering](docs/VISION_AND_PARADIGM.md)
- [Existing Landscape & Comparative Analysis](docs/EXISTING_LANDSCAPE.md)
- [System Architecture](docs/ARCHITECTURE.md)
- [Configuration Specification (`hardgate.toml`)](docs/CONFIGURATION_SPEC.md)
- [CLI Reference & Agent Integration](docs/CLI_AND_INTEGRATION.md)
