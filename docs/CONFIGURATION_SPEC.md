# Configuration specification

Hardgate reads `hardgate.toml` from the current directory. If the file is absent, the CLI uses the `strict-agent` default bundle. `hardgate init --preset …` writes an explicit template; that template leaves coverage, mutation, orchestration, and dead-code execution disabled until a project supplies the required inputs.

## Minimal configuration

```toml
[gate]
name = "my-service"
preset = "strict-agent"
strict = true
```

`preset` accepts `strict-agent`, `balanced`, `legacy-migration`, or `custom`. Presets provide base values and a present section/key in the file overrides those values. `custom` starts from the deserialized defaults.

## Gate policy and evidence

```toml
[gate]
name = "my-service"
preset = "strict-agent"
strict = true
enforce_classified_sources = false
```

- `strict = true` turns missing/unreadable required evidence, parser failures, Git failures, and unsupported classified source files into blocking findings. With `strict = false`, those evidence failures are visible advisories; ordinary engine violations still fail.
- `enforce_classified_sources = true` fails unknown inventory files instead of allowing them through without a role. It does not add parsers for unsupported formats.

The coverage and mutation switches are independent. A disabled switch does not read, parse, or score a configured stale report. An enabled switch requires a report path and, in strict mode, fails when the report is absent or malformed.

## Discovery roles and formats

Every discovered inventory file receives one role before engines select inputs:

| Role | Current policy |
| --- | --- |
| `source` | File/anti-gaming/invariant checks; AST complexity when a parser exists; native mutation target when supported |
| `test` | File/anti-gaming/invariant checks; AST complexity; clone analysis; never a native mutation target |
| `fixture` | Size/anti-gaming safety and clone analysis; no AST complexity by default |
| `generated` | Inventory advisory only; no handwritten complexity or clone debt |
| `migration` | Size/anti-gaming safety; unsupported formats fail in strict mode when classified as source-like |
| `config` | Size/anti-gaming safety |
| `documentation` | Inventory only |
| `vendor` | Pruned with build/dependency directories |
| `unknown` | Passes unless `enforce_classified_sources` is enabled |

Built-in pruning covers `node_modules`, `target`, `dist`, `build`, `vendor`, `.venv`, `venv`, and `__pycache__`. User exclusions are not pruning: they remain visible as advisories and only belong to the engine whose exclusion list contains them.

AST parsing is available for `.rs`, `.js`, `.jsx`, `.mjs`, `.cjs`, `.ts`, `.tsx`, `.mts`, `.cts`, `.py`, and `.go`. Inventory-only formats include `.css`, `.mdx`, `.sql`, `.json`, `.jsonc`, `.graphql`, `.gql`, `.snap`, `.toml`, `.yaml`, and `.yml`.

## File and function budgets

```toml
[budgets.files]
max_bytes = 32768

[budgets.files.max_lines]
rs = 499
ts = 400
tsx = 400
js = 400
jsx = 400
py = 400
go = 400
default = 350

[budgets.files.exclusions]
paths = ["src/generated/**"]

[budgets.functions]
max_cyclomatic = 10
max_cognitive = 15
max_halstead_difficulty = 80.0
max_abc = 100.0
max_parameters = 4
max_lines = 80
max_statements = 30
max_nesting_depth = 4
```

File limits use raw bytes and physical lines. Function limits come from the Tree-sitter metrics for supported parser targets. A file-budget exclusion skips only byte/line checks and emits an advisory; it does not suppress anti-gaming, invariant, parsing, or clone checks.

## Anti-gaming scanner

```toml
[anti_gaming]
disallow_suppressions = true
custom_forbidden_tokens = ["NOLINT"]
```

The scanner checks known suppression directives in comments/attributes and project-provided literal tokens. When `disallow_suppressions` is false, the scanner does not emit suppression findings. The current configuration has no per-file exception path.

## Architectural invariants

```toml
[invariants]
enforce = true

[[invariants.rules]]
name = "UI boundary"
from = "src/components/**"
disallow_imports = ["@tauri-apps/api*"]
message = "Route native calls through the domain service."

[[invariants.rules]]
name = "No direct fetch"
from = "src/**"
exclude = ["src/lib/network.ts"]
disallow_calls = ["fetch"]
```

Rules are declarative line-level checks for import strings, call names, or tokens. They do not resolve modules or perform compiler type checking. `from` and `exclude` are glob patterns.

## Clone detection

```toml
[clones]
enabled = true
min_lines = 5
min_tokens = 50
excludes = ["tests/fixtures/**"]
```

The detector tokenizes eligible source, test, and fixture text, normalizes literal values, and compares bounded rolling-hash windows. `excludes` belongs only to clone detection and produces an advisory when matching files are present. It is not a general discovery ignore.

## LCOV coverage and CRAP evidence

```toml
[coverage]
enabled = false
report = "coverage/lcov.info"
min_line_percent = 95.0
min_function_percent = 95.0
min_branch_percent = 90.0
max_crap_score = 25.0
critical_paths = ["src/core.ts"]
```

The current parser accepts LCOV records. When enabled, Hardgate evaluates global line/function/branch floors, per-function CRAP, and optional critical paths. A function without matching report lines is treated as uncovered for its CRAP calculation; a source file with analyzed functions absent from the report is a missing-evidence finding. No other coverage report format is read in this revision.

## Mutation policy and reports

```toml
[mutation]
enabled = false
min_score = 85.0
reject_timeouts = true
timeout_secs = 10
max_mutants = 30
test_cmd = "pnpm test {file}"
reports = ["reports/stryker-mutation.json"]
```

`verify` and the evidence phase of `check` evaluate Stryker-shaped JSON (`files → mutants`), cargo-mutants-shaped JSON (`outcomes`), or generic outcome-count JSON. They do not run those tools. `reject_timeouts` controls timeout findings; compile, runner, and unviable outcomes are always integrity findings. Scores use killed divided by killed plus survived; a report with no viable mutants scores 0% and therefore fails the usual positive floor.

`hardgate mutate` is separate. It mutates classified production sources with the built-in AST mutator, runs the configured or inferred test command, executes an unmutated baseline before mutants, rejects zero viable mutation points, and verifies byte-for-byte restoration. Use `--test-cmd` for a project runner that is not inferred by the file extension.

## Orchestration

```toml
[orchestration]
format_check = "cargo fmt --check"
format = "cargo fmt"
lint = "cargo clippy -- -D warnings"
test_cmd = "cargo test"
```

`hardgate fmt --check` runs `format_check`; `hardgate fmt` runs `format` (or falls back to `format_check`). `hardgate check --all` runs each configured format-check, lint, and test command in addition to static engines. Commands are executed from the repository root; a local `node_modules/.bin` is prepended to `PATH`.

## Dead-code analysis

```toml
[analysis.dead_code]
enabled = false
entry_points = ["src/main.rs", "src/lib.rs"]
exclude = ["tests/**"]
```

Dead-code analysis is enabled by this section or explicitly with `check --dead-code`. It reports unreferenced files and simple unused JavaScript/TypeScript exports using configured entry/exclusion globs; it is not a compiler linker or whole-program proof.

## Presets

| Preset | Current behavior |
| --- | --- |
| `strict-agent` | Tight budgets, suppression scanning, clone detection, and strict evidence handling. The no-config fallback enables coverage and mutation report checks; an initialized file explicitly disables them until configured. |
| `balanced` | Scaled structural budgets and strict ordinary findings; coverage/mutation are disabled by the preset bundle. |
| `legacy-migration` | Scaled structural budgets and non-strict evidence handling; no merge-base baseline or ratchet is implemented. |
| `custom` | Uses values explicitly present in the file and deserialized defaults. |

**Planned stabilization (not current behavior):** merge-base baselines and a `legacy-migration` ratchet may be enabled after their implementation and regression tests land. Until then, do not describe this preset as comparing or preventing regressions against a reference branch.
