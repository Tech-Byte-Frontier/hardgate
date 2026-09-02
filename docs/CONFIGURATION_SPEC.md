# Configuration Specification (`hardgate.toml`)

The configuration file for Hardgate is placed at the root of a project as `hardgate.toml`.

---

## 1. Minimal Example

```toml
[gate]
name = "my-awesome-service"
preset = "strict-agent"
```

The `strict-agent` preset applies all standard Loreframe-level hard budgets out of the box.

---

## 2. Comprehensive Specification

```toml
# ==============================================================================
# 1. CORE GATE SETTINGS
# ==============================================================================
[gate]
name = "loreframe"
# Preset: "strict-agent" | "balanced" | "lenient" | "custom"
preset = "custom"

# In strict mode, baselines, ignored source paths, or warnings are treated as errors.
strict = true

# Fail the gate if files exist that do not match the defined source classifications.
enforce_classified_sources = true

# ==============================================================================
# 2. PHYSICAL & FILE SIZE BUDGETS
# ==============================================================================
[budgets.files]
# Maximum physical byte size for any handwritten source file (e.g. 32 KiB)
max_bytes = 32768

# Maximum physical line counts per file extension
[budgets.files.max_lines]
rs = 499
ts = 400
tsx = 400
js = 400
jsx = 400
py = 400
go = 400
css = 400
default = 350

# Files excluded from line/byte size budgets (e.g., generated schema types)
[budgets.files.exclusions]
paths = [
  "src/vite-env.d.ts",
  "src-tauri/src/schema.rs"
]

# ==============================================================================
# 3. FUNCTION COMPLEXITY BUDGETS
# ==============================================================================
[budgets.functions]
# McCabe Cyclomatic Complexity threshold
max_cyclomatic = 10

# Cognitive Complexity threshold (Sonar model)
max_cognitive = 15

# Halstead difficulty threshold
max_halstead_difficulty = 80.0

# ABC Metric (Assignments, Branches, Conditions) threshold
max_abc = 100.0

# Maximum formal parameters allowed in a function signature
max_parameters = 4

# Maximum physical lines inside a single function body
max_lines = 80

# Maximum statement count in a single function
max_statements = 30

# Maximum nesting depth of control flow blocks (if/loop/match)
max_nesting_depth = 4

# ==============================================================================
# 4. ANTI-GAMING DIRECTIVES (ZERO-SUPPRESSION)
# ==============================================================================
[anti_gaming]
# When true, rejects all suppression comments across all languages
disallow_suppressions = true

# Standard scanned patterns include:
# - TypeScript/JS: @ts-ignore, @ts-nocheck, eslint-disable, oxlint-disable, prettier-ignore
# - Rust: #[allow(...)], #[expect(...)], mutants::skip, coverage(off)
# - Python: # type: ignore, # noqa, # pragma: no cover
# - Coverage: c8 ignore, istanbul ignore, v8 ignore

# Additional project-specific forbidden suppression tokens
custom_forbidden_tokens = [
  "LIZARD_FORGIVES",
  "NOLINT"
]

# ==============================================================================
# 5. ARCHITECTURAL INVARIANTS & BOUNDARIES
# ==============================================================================
[invariants]
enforce = true

[[invariants.rules]]
name = "Tauri IPC Isolation"
from = "src/components/**"
disallow_imports = ["@tauri-apps/api*", "@tauri-apps/plugin*"]
message = "Components must invoke native actions via src/lib/studio-api.ts."

[[invariants.rules]]
name = "Direct Fetch Ban"
from = "src/**"
exclude = ["src/lib/network.ts"]
disallow_calls = ["fetch"]
message = "Direct fetch is prohibited; use src/lib/network.ts."

[[invariants.rules]]
name = "No Unsafe Rust"
from = "src-tauri/src/**"
disallow_tokens = ["unsafe"]
message = "Unsafe Rust blocks are forbidden in this codebase."

# ==============================================================================
# 6. CODE DUPLICATION & CLONE DETECTION
# ==============================================================================
[clones]
enabled = true
min_lines = 5
min_tokens = 50
excludes = [
  "**/tests/**",
  "**/*_test.rs",
  "**/*.spec.ts"
]

# ==============================================================================
# 7. COVERAGE & CRAP RISK
# ==============================================================================
[coverage]
enabled = true
# Path to LCOV, Cobertura, or JSON coverage report
report = "coverage/lcov.info"

# Global coverage floor
min_line_percent = 95.0
min_function_percent = 95.0
min_branch_percent = 95.0

# Per-function CRAP (Change Risk Anti-Patterns) threshold
max_crap_score = 25.0

# Critical files requiring 100% coverage across all metrics
critical_paths = [
  "src/lib/studio-api.ts",
  "src-tauri/src/importer.rs",
  "src-tauri/src/credentials.rs"
]

# ==============================================================================
# 8. MUTATION TESTING FLOOR
# ==============================================================================
[mutation]
enabled = true
# Minimum mutation score floor (killed / total viable mutants)
min_score = 85.0
# Reject runs containing timeouts, syntax errors, or unviable baselines
reject_timeouts = true
# Scoped and full mutation report locations
reports = [
  "reports/stryker-mutation.json",
  "reports/cargo-mutants.json"
]
```

---

## 3. Standard Presets

| Preset | Target Use Case | Key Thresholds |
| :--- | :--- | :--- |
| `strict-agent` *(Default)* | Autonomous AI Agent projects | Cyclo $\le 10$, Cogn $\le 15$, Zero-suppression active, File lines $\le 400$, CRAP $< 25$, Mutation $\ge 85\%$. |
| `balanced` | Human-AI hybrid teams | Cyclo $\le 15$, Cogn $\le 20$, Suppressions require signed justification, File lines $\le 600$, CRAP $< 30$. |
| `legacy-migration` | Existing large codebases | Enables ratcheting mode: metrics cannot worsen from `git merge-base`. |
