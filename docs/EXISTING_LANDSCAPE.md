# Existing Landscape & Comparative Analysis

To understand why a dedicated tool like **Hardgate** is needed, we must survey the current software quality, code analysis, and agent verification landscape.

---

## 1. Survey of Existing Solutions

### A. PMAT (Pragmatic Multi-language Agent Toolkit)
* **Crate:** `pmat` (`cargo install pmat`)
* **Focus:** AI-agent development context & technical debt grading
* **Overview:** Developed by PAIML, PMAT is a Rust-based tool and Model Context Protocol (MCP) server that grades codebases using a "Technical Debt Gradient" (A+ to F scale). It features an autonomous pre-flight command (`pmat verify`) and quality gate enforcement (`pmat quality-gate --strict`).
* **Strengths:** Native MCP integration, multi-language support (20+ languages), includes mutation testing concepts and git history RAG.
* **Limitations:** Primarily geared toward high-level technical debt grading and LLM context generation rather than strict physical line/byte budgets, granular zero-suppression regex/AST scanning, or declarative architectural boundary enforcement.

### B. BCA (big-code-analysis-cli)
* **Crate:** `big-code-analysis-cli` (`cargo install big-code-analysis-cli`)
* **Focus:** High-speed multi-language code metrics via Tree-sitter
* **Overview:** A Rust fork of Mozilla's `rust-code-analysis`. Computes Cyclomatic Complexity, Cognitive Complexity, Halstead metrics, Maintainability Index, ABC, and LOC across 20+ languages without invoking compilers or runtimes. Features `bca check --strict --no-suppress`.
* **Strengths:** Extremely fast, language runtime independent, Tree-sitter powered, explicitly designed for agent feedback.
* **Limitations:** Purely a metric calculator. It does not run test suites, does not ingest or compute CRAP scores from coverage files, does not detect clone blocks across files, and does not enforce custom import/architectural invariants.

### C. Trunk Check (`trunk.io`)
* **Platform:** Proprietary / open-core CLI (Go/Rust binary wrapper)
* **Focus:** Multi-linter orchestration with hermetic runtimes
* **Overview:** Trunk manages and executes 100+ linters (Clippy, ESLint, Ruff, Prettier, etc.) locally and in CI using pinned hermetic binaries.
* **Strengths:** Excellent developer experience for orchestrating external linters; git-aware caching.
* **Limitations:** Trunk is an *orchestrator of external tools*, not a semantic quality gate. It cannot compute cross-language AST budgets, does not enforce zero-suppression policies across linters, and has no native concept of mutation scores or architectural boundaries.

### D. Lefthook / pre-commit
* **Platform:** Go (`lefthook`) / Python (`pre-commit`)
* **Focus:** Git hook task runners
* **Overview:** Fast command runners that trigger linters or formatters before git commits or pushes.
* **Strengths:** Extremely fast, concurrent execution.
* **Limitations:** Generic runners with zero domain knowledge. They execute whatever shell commands are defined, meaning teams still have to write and maintain complex glue scripts.

### E. SonarQube / SonarCloud
* **Platform:** Java daemon / Cloud SaaS
* **Focus:** Enterprise Static Analysis & Clean as You Code Quality Gates
* **Overview:** The enterprise standard for quality gates, cognitive complexity, and duplication detection.
* **Strengths:** Very mature metrics, established quality gate philosophy.
* **Limitations:** Heavyweight, Java-dependent, slow execution times, centralized server architecture. Completely unusable for local, sub-second agent feedback loops where an agent needs immediate verification after every file edit.

---

## 2. The Current Reality: Bespoke Glue Scripts

Because no single tool bridges AST complexity, hard budgets, anti-gaming suppression rules, coverage/CRAP calculation, mutation floors, and architectural invariants, cutting-edge projects are forced to build **custom glue scripts**.

For example, Loreframe's verification system relies on:
- `scripts/quality-gate.mjs` (272 lines of orchestration logic)
- `scripts/quality-policy.mjs` (253 lines of file classification and suppression detection)
- `scripts/quality-invariants.mjs` (architectural boundary checking)
- Pinned external binaries: `oxlint`, `oxfmt`, `bca`, `cargo-llvm-cov`, `cargo-mutants`, `stryker`, `knip`, `jscpd`, `cargo-machete`.

While this setup achieves incredible rigor, **it requires hundreds of lines of brittle Node.js glue code and dozens of package dependencies that cannot be easily shared across other repositories.**

---

## 3. Feature Comparison Matrix

| Capability | PMAT | BCA | Trunk Check | Lefthook | SonarQube | **Hardgate** |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: |
| **Language Runtime** | Rust | Rust | Go/Rust | Go | Java | **Rust** |
| **Zero Runtime Deps (Static Binary)** | Yes | Yes | Partial | Yes | No | **Yes** |
| **Sub-Second Execution** | Yes | Yes | Medium | Fast | Slow | **Ultra-Fast** |
| **Tree-sitter AST Complexity (20+ langs)** | Partial | Yes | No | No | Custom | **Yes** |
| **Hard Physical Budgets (Lines/Bytes)** | No | Limited | No | No | No | **Yes** |
| **Zero-Suppression Anti-Gaming Scan** | No | Basic | No | No | No | **Strict AST+Regex** |
| **Architectural Boundary Linter** | No | No | No | No | Architecture | **Declarative AST** |
| **Built-in Clone Detection (Zero-dep)** | No | No | External | No | Yes | **Token Hash** |
| **CRAP Score & Coverage Ingestion** | Basic | No | No | No | Partial | **Yes (LCOV/JSON)** |
| **Mutation Testing Gatekeeper** | Metrics | No | No | No | No | **Floor Enforcement** |
| **Agent-Optimized Token Output** | Yes | Yes | No | No | No | **Yes** |
| **Native MCP Server Mode** | Yes | No | No | No | No | **Yes** |

---

## 4. Why Hardgate Fills the Gap

Hardgate is designed specifically for the **post-LLM software era**:
1. **Single binary installation:** `cargo install hardgate` gives you an instant, zero-dependency quality gate for Rust, TypeScript, Python, Go, and C++ codebases.
2. **Anti-gaming by default:** Prevents agents from silently adding `@ts-ignore`, `#[allow(...)]`, or `# noqa`.
3. **All-in-one efficiency:** Combines the metric power of `BCA`, the duplication checks of `jscpd`, the architectural checks of `dependency-cruiser`, and the verification loops of `PMAT` into one unified CLI.
