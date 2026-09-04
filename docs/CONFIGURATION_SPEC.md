# Configuration specification

Hardgate reads `hardgate.toml` from the current directory. If the file is absent, `HardgateConfig::load_or_default` uses the `strict-agent` preset object. `hardgate init --preset …` serializes that same preset object as a commented template.

## Presets and presence-based merging

```toml
[gate]
name = "my-service"
preset = "strict-agent"
strict = true
```

`preset` accepts `strict-agent`, `balanced`, `legacy-migration`, or `custom`.

- `strict-agent` supplies tight structural budgets, strict static/classification fallback, and enabled coverage/mutation report policies with their configured floors. Coverage defaults to `coverage/lcov.info`; mutation is enabled but requires a report path in TOML (`verify --mutation-report <path>` can supply one for that command).
- `balanced` scales structural budgets and disables coverage/mutation report policies.
- `legacy-migration` scales structural budgets, disables coverage/mutation report policies, and enables a static reference/merge-base ratchet. It defaults to `reference_branch = "origin/main"` and `strict = false`.
- `custom` uses values explicitly present in the file plus serde defaults.

For every non-custom preset, merging is presence-based. Hardgate inspects the TOML table and overlays only keys that are actually present; omitted sections and keys retain the preset value. An explicit `false`, empty array, or other explicit value is not treated as omission. This lets a project change one field without copying the rest of the preset.

The `strict` flag controls static/classification evidence fallback: parser/read
failures and similar static evidence can be blocking (`true`) or advisories
(`false`) when no role-specific severity overrides them. An unknown-role gap is
always blocking when `gate.enforce_classified_sources = true`, regardless of
`strict`; the flag applies to other evidence without a role override.
Explicitly enabled coverage, mutation, generated-freshness, and legacy-reference
evidence is required and blocking regardless of `strict`.

## Gate identity

```toml
[gate]
name = "my-service"
preset = "strict-agent"
strict = true
enforce_classified_sources = false
```

- `name` labels reports.
- `preset` selects the base bundle.
- `strict` controls fallback severity for static evidence without a role policy.
- `enforce_classified_sources = true` turns an unknown inventory file into a classification finding. It does not add an AST parser.

## Discovery, classification, and role policies

Each inventory file receives one role before engines choose inputs. Built-in pruning always skips `node_modules`, `target`, `dist`, `build`, `vendor`, `.venv`, `venv`, and `__pycache__`. File-budget and clone exclusions are not global pruning: excluded files remain available to classification and other engines, and the owning engine emits an advisory. Dead-code exclusions are local to that analyzer and silent.

Built-in role behavior:

| Role | Default engines and targets |
| --- | --- |
| `source` | File/anti-gaming/invariant checks, AST complexity when supported, clone analysis, native mutation target |
| `test` | File/anti-gaming/invariant checks, AST complexity, clone analysis; never a native mutation target |
| `generated` | Inventoried and reported as generated; no handwritten complexity or clone debt by default |
| `fixture` | File/anti-gaming safety and clone analysis; no AST complexity by default |
| `migration` | File/anti-gaming safety; no native mutation or clone analysis by default |
| `config` | File/anti-gaming safety |
| `documentation` | Inventory visibility only |
| `vendor` | Pruned dependency/build output |
| `unknown` | No role-specific engine input; fails when `enforce_classified_sources` is enabled |

Tree-sitter targets are `.rs`, `.js`, `.jsx`, `.mjs`, `.cjs`, `.ts`, `.tsx`, `.mts`, `.cts`, `.py`, and `.go`. Inventory-only formats are `.css`, `.mdx`, `.sql`, `.json`, `.jsonc`, `.graphql`, `.gql`, `.snap`, `.toml`, `.yaml`, and `.yml`.

### Node and Supabase conventions

The JavaScript-family extensions (`.js`, `.jsx`, `.mjs`, `.cjs`, `.ts`,
`.tsx`, `.mts`, and `.cts`) are parser-supported when the classified role is
source or test. Built-in Supabase conventions classify
`supabase/database.types.ts` and `supabase/schema.gen.ts` as generated and
`supabase/functions/**/*.ts` as source. `supabase/migrations/**/*.sql`,
`supabase/seed.sql`, and `*.migration.sql`/`*.seed.sql` are migration files
without an AST parser; `supabase/seed.ts` is also migration-role but has
TypeScript parser support. Migrations remain inventoried and receive migration
safety checks rather than ordinary source/test complexity or native mutation.
With the default strict migration policy, parser-unsupported migration files
produce a blocking `unsupported-source` finding. A custom rule may assign
another role, but classification does not add SQL metrics. Supabase
configuration/data such as `supabase/config.toml` is configuration inventory
and has no function metrics.

Ordered custom rules run before built-ins, except that vendor/build pruning remains authoritative:

```toml
[classification]

[[classification.rules]]
glob = "supabase/functions/**"
role = "source"
```

The first matching rule wins. Invalid or duplicate globs fail configuration loading.

### Independent role policies

```toml
[roles.source]
severity = "error"
max_lines = 499
max_cyclomatic = 10
clone_enabled = true # explicitly enable this role, even when [clones].enabled = false
clone_min_lines = 5
clone_min_tokens = 50
mutation_target = true

[roles.test]
severity = "warning"
max_function_lines = 120
clone_min_lines = 8
clone_min_tokens = 80
mutation_target = false

[roles.generated]
severity = "ignore"
clone_enabled = false
mutation_target = false

[roles.fixture]
severity = "warning"
clone_enabled = true

[roles.migration]
severity = "error"
clone_enabled = false
mutation_target = false
```

The five first-class sections (`source`, `test`, `generated`, `fixture`, `migration`) are independent. `severity` is `error`, `warning`, or `ignore`; omitted thresholds inherit global budgets. Role policy can override file bytes/lines, function ceilings, clone enablement/thresholds, and native mutation eligibility. A role cannot opt a non-source file into native mutation.

`clone_enabled` is tri-state: `true` explicitly enables clone analysis for that role, `false` disables it, and an omitted key inherits `[clones].enabled`. Presets leave `source`, `test`, and `fixture` omitted so the global clone setting remains the master-like default; `generated` and `migration` are explicitly disabled.

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

File limits use raw bytes and physical lines. Function limits come from Tree-sitter metrics for supported parser targets. `[budgets.files.exclusions].paths` skips only byte/line checks and emits an advisory; it does not suppress anti-gaming, invariants, parsing, clones, role classification, or generated freshness.

## Anti-gaming checks

```toml
[anti_gaming]
disallow_suppressions = true
custom_forbidden_tokens = ["NOLINT"]
```

The scanner recognizes common compiler, linter, type-checker, and coverage suppression directives plus literal project tokens in safety-checked roles. `disallow_suppressions = false` disables those findings. There is no per-file approval channel.

## Architectural invariants

`[invariants].enforce` defaults to `true`; an empty `rules` list is simply a no-op. Set it to `false` to disable invariant checks explicitly.

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

Rules are declarative line-level checks for import strings, call names, or tokens. `from` and `exclude` are globs. The checker does not resolve modules or perform compiler/type analysis.

## Clone detection

```toml
[clones]
enabled = true
min_lines = 5
min_tokens = 50
excludes = ["tests/fixtures/**"]
```

Eligible source, test, and fixture files are analyzed in separate role groups using normalized lexical token streams and bounded rolling-hash windows. `excludes` belongs only to clone detection and emits an advisory when matching files are present. In `check --diff`, Git-changed/staged inventory is selected by default, explicit existing paths add to static/clone selection, and Hardgate indexes the full repository to retain clone pairs touching Git-changed/staged files or explicitly selected existing paths. Every current clone violation has a stable fingerprint over normalized token kinds; it excludes paths and physical line numbers, allowing rename lineage to preserve identity.

## Generated-artifact freshness

```toml
[generated]
enabled = true
freshness_command = "sh -c 'pnpm generate && git diff --exit-code -- generated/'"
timeout_secs = 300
```

When enabled, `freshness_command` is required and runs in `check` (including `--diff`) and `verify`. A missing command, timeout, non-zero exit, or runner failure is blocking current evidence. Freshness has its own timeout and is independent of `[budgets.files.exclusions]`; excluding generated files from a size check never disables freshness. Freshness is not part of the legacy static ratchet.

Configured commands are quote-aware tokenized arguments launched directly; Hardgate does not invoke an implicit shell. Shell operators such as `&&`, pipes, and redirection are ordinary arguments unless the command explicitly invokes a shell, for example `sh -c 'command-a && command-b'`.

## Legacy reference and ratchet

```toml
[legacy]
reference_branch = "origin/main"
ratchet = true
```

`ratchet = true` requires a non-empty reference. Hardgate resolves the Git merge base, loads the baseline snapshot, and analyzes baseline static findings plus configured dead-code findings. Existing non-worsened static debt can be grandfathered as advisories; new or worsened findings with effective role severity `error` remain blocking, `warning` findings remain advisories, and `ignore` findings are omitted. Pure rename lineage maps the current path back to the baseline path. Stable clone fingerprints remove line-number dependence. Retained findings are annotated with changed files or changed hunk ranges.

The ratchet applies only to static and configured dead-code findings. Coverage, mutation, generated freshness, and orchestration are evaluated against the current tree and remain blocking; they are never grandfathered. If the reference, merge base, snapshot, or baseline analysis cannot be loaded, the ratchet reports a blocking evidence failure.

With the ratchet enabled, `check --diff` still uses actual Git-changed
executable lines for LCOV, while static and clone analysis disables diff
filtering but honors explicit existing paths added to the selected static/clone
scope: the selected scope is the full current tree when no paths are supplied.
The ratchet still loads and validates the full configured reference snapshot,
then compares it only to selected current static/dead-code findings. Without a
ratchet, Git-changed/staged inventory is the default and explicit existing
paths add to static/clone selection.

## Coverage and CRAP evidence

```toml
[coverage]
enabled = true
report = "coverage/lcov.info"
min_line_percent = 95.0
min_function_percent = 95.0
min_branch_percent = 90.0
max_crap_score = 25.0
critical_paths = ["src/core.ts"]
```

Only LCOV is parsed. Full checks evaluate global line/function/branch floors, function CRAP scores, critical paths, and missing source records. `check --diff` filters Git changes to actual changed executable lines in AST-supported source-role files and reports uncovered lines or missing file records. `check`, `check --all`, and `verify` resolve the report the same way: an explicit CLI path takes precedence over `coverage.report`; neither command auto-discovers conventional report filenames. A missing path, empty, unreadable, or malformed report is blocking whenever coverage is enabled, regardless of `gate.strict`.

This repository's self-gate generates branch LCOV with the pinned
`RUST_COVERAGE_TOOLCHAIN` (`nightly-2026-09-04`) because Rust branch
instrumentation is unstable. The producer-only nightly toolchain does not
change the Rust 1.98.1 crate MSRV or normal build/test gates; the helper
includes the executable `build.rs` in that LCOV report.

`verify` accepts optional path arguments for the current static/dead-code
inventory and coverage source matching only. Mutation-report ingestion and
generated freshness remain configured/full. The ratchet still loads and
validates the full configured reference snapshot, then compares it only to the
selected current static/dead-code findings; explicit paths do not widen that
current selection.

## Mutation report evidence

```toml
[mutation]
enabled = true
min_score = 85.0
timeout_secs = 10
max_mutants = 30
test_cmd = "pnpm test {file}"
reports = ["reports/stryker-mutation.json"]
```

`check` and `verify` evaluate Stryker-shaped (`files`), cargo-mutants-shaped (`outcomes`), or generic outcome-count JSON. Empty reports, empty outcome arrays, missing reports, parse errors, and reports with no viable outcomes are blocking when mutation is enabled. Scores use killed divided by killed plus survived. Timeout, compile-error, runner-error, and unviable outcomes are integrity findings and remain blocking; mutation timeout handling is not a user-weakenable exception.

`hardgate mutate` is separate native execution. It does not read `reports` and does not invoke an external mutation tool. When `[mutation].enabled = false`, it prints a disabled-policy note and exits successfully without target discovery or execution; the native baseline and no-target rules apply only when enabled.

Native mutation is available to source builds on Linux and macOS through the
target-OS cfg. On other operating systems it fails closed before baseline or
source writes because robust process-group cleanup and atomic source restoration
are not available there. This limitation applies to native mutation; static
`check` and `scan` remain separate capabilities. The prebuilt, npm, and
shell-installer release contract remains exactly six x64/arm64 glibc/musl/macOS
targets (Linux x64/arm64 glibc and musl, macOS x64/arm64), which does not
constrain source builds.

After an explicit scope is validated, a `mutate --diff` run (including
`--scoped`) with no changed production source is a successful no-op. Missing,
invalid, unsupported, or non-source explicit scopes fail closed; only a non-diff
unrestricted or scoped run with no eligible source target fails.

## Orchestration and dead code

```toml
[orchestration]
format_check = "cargo fmt --check"
format = "cargo fmt"
lint = "cargo clippy -- -D warnings"
test_cmd = "cargo test"
timeout_secs = 300

[analysis.dead_code]
enabled = false
entry_points = ["src/main.rs", "src/lib.rs"]
exclude = ["tests/**"]
```

`entry_points` globs are additive to the analyzer's built-in safe roots; an empty list does not clear those built-ins. `exclude` globs are additive to built-in test exclusions. `check --all` runs configured format-check, lint, and test commands. `fmt --check` uses `format_check`; `fmt` uses `format`, falling back to `format_check`. Commands run from the repository root with local `node_modules/.bin` available on `PATH`. Dead-code analysis is enabled by policy or `check --dead-code`; it reports unreferenced files and simple JS/TS exports, not a compiler linker proof.

## Validation and fail-closed rules

Serde handles types and enum values; semantic validation rejects non-positive thresholds, invalid/duplicate globs (including invariant import globs and dead-code entry-point globs), enabled freshness without a command, enabled legacy ratchet without a reference, and unsafe mutation settings. Empty required reports/outcomes, unreadable files, parser failures, Git failures, and configured command failures are never silently converted into a pass. The CLI retains an advisory when source discovery is empty and still evaluates enabled evidence; MCP `hardgate_check` rejects empty scopes/discovery explicitly.
