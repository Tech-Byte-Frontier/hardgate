# Configuration specification

Hardgate finds the nearest `hardgate.toml` while searching upward to the first
Git boundary. `--config FILE` selects an explicit policy; missing or invalid
explicit files fail. Without a discovered policy, strict-agent defaults apply.
`hardgate init` creates a balanced policy by default; it never replaces an
existing policy. This initialization choice does not change no-config fallback.
Policy paths use the configuration root, while CLI paths use the invocation
directory. `hardgate config` displays the merged, validated effective policy.
See [configuration authority](CLI_AND_INTEGRATION.md) for monorepo details.

## Presets and presence-based merging

```toml
[gate]
name = "my-service"
preset = "strict-agent"
strict = true
```

`preset` accepts `strict-agent`, `balanced`, `legacy-migration`, or `custom`.

- `strict-agent` supplies tight structural budgets, strict static/classification fallback, and enabled coverage/mutation report policies with their configured floors. Coverage defaults to `.hardgate/evidence/coverage.lcov`; mutation is enabled but requires a report path in TOML (`check --mutation-report <path>` can supply one for that command).
- `balanced` scales structural budgets and disables coverage/mutation report policies.
- `legacy-migration` scales structural budgets, disables coverage/mutation report policies, and enables a static reference/merge-base ratchet. It defaults to `reference_branch = "origin/main"` and `strict = false`.
- `custom` uses values explicitly present in the file plus serde defaults.

The preset ceilings below apply to handwritten production code. Values are
policy choices, not proof that every finding warrants a refactor.

| Budget | strict-agent | balanced / legacy-migration |
| --- | --- | --- |
| Cyclomatic complexity | 10 | 15 |
| Nesting / parameters | 4 / 5 | 6 / 6 |
| Function lines / statements | 80 / 30 | 120 / 50 |
| File lines: Rust / JS, TS / fallback | 499 / 400 / 350 | 600 / 500 / 500 |
| File bytes | 32,768 | 65,536 |
| Coverage: lines, functions, branches | Required: 95%, 95%, 90% | Disabled; configured floors 80%, 80%, 75% |
| Mutation score | Required: 85% | Disabled; configured floor 75% |

All non-custom presets keep tests in analysis. Test file size, function lines,
statement counts, and duplication are advisories by default; other test
complexity, anti-gaming, invariants, and parser/classification failures retain
error severity. Native mutation still targets source only.

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
| `source` | File/anti-gaming/invariant checks, AST complexity when supported, clone analysis, mutation evidence source |
| `test` | File/anti-gaming/invariant checks, AST complexity, clone analysis; never a mutation evidence source |
| `generated` | Inventoried and reported as generated; no handwritten complexity or clone debt by default |
| `fixture` | File/anti-gaming safety and clone analysis; no AST complexity by default |
| `migration` | File/anti-gaming safety; no mutation targeting or clone analysis by default |
| `config` | File/anti-gaming safety |
| `documentation` | Inventory visibility only |
| `vendor` | Pruned dependency/build output |
| `unknown` | No role-specific engine input; fails when `enforce_classified_sources` is enabled |

Tree-sitter targets are `.rs`, `.js`, `.jsx`, `.mjs`, `.cjs`, `.ts`, `.tsx`, `.mts`, and `.cts`. Inventory-only formats are `.css`, `.mdx`, `.sql`, `.json`, `.jsonc`, `.graphql`, `.gql`, `.snap`, `.toml`, `.yaml`, and `.yml`.

Rust and JavaScript/TypeScript are the only supported analysis ecosystems.
Python/Go parsers and tool adapters are removed. Their source files are retained
only as unsupported-input failures, so required analysis cannot silently pass.
Remove obsolete `budgets.files.max_lines.py` and `.go` settings; loading them
fails explicitly. Configuration, documentation, assets and generated-file
inventory do not imply additional ecosystem support.


Inventory-only does not mean silently accepted. A file that remains in the
source or migration role without parser support is an `unsupported-source`
finding; the preset severities make it blocking. Projects must reclassify such
a file only when another role is truthful, or deliberately change the
applicable role policy. Classification never adds parser support.

Rust files named `tests.rs`, `*_tests.rs`, or `*-tests.rs` use the built-in
test role convention; the suffix rule is limited to `.rs` so similarly named
JavaScript and other files remain source unless another convention
matches. The production JavaScript test selector is source-named and remains
eligible for coverage.

Rust inline `#[cfg(test)]` regions and `#[test]` items use test policy while
preserving their original locations. Normal and `#[path]` module declarations
propagate test ownership to helper files when every known importer is test-only.
Cargo targets, production importers, malformed context, and unresolved module
macros prevent unsupported test-only claims. Custom classification rules remain
authoritative. This is syntactic ownership, not macro expansion or reachability.
Coverage scoring removes mapped test lines/functions/branches from production
counts; incomplete details or mixed production/test lines block that projection.
Cargo-mutants evidence rejects mutations in test-only files or spans.

### Node and Supabase conventions

The JavaScript-family extensions (`.js`, `.jsx`, `.mjs`, `.cjs`, `.ts`,
`.tsx`, `.mts`, and `.cts`) are parser-supported when the classified role is
source or test. Built-in Supabase conventions classify
`supabase/database.types.ts` and `supabase/schema.gen.ts` as generated and
`supabase/functions/**/*.ts` as source. `supabase/migrations/**/*.sql`,
`supabase/seed.sql`, and `*.migration.sql`/`*.seed.sql` are migration files
without an AST parser; `supabase/seed.ts` is also migration-role but has
TypeScript parser support. Migrations remain inventoried and receive migration
safety checks rather than ordinary source/test complexity .
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
clone_block_min_lines = 10
clone_block_min_tokens = 100
mutation_target = true

[roles.test]
severity = "error"
file_size_severity = "warning"
function_size_severity = "warning"
clone_severity = "warning"
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

The five first-class sections (`source`, `test`, `generated`, `fixture`, `migration`) are independent. `severity` is `error`, `warning`, or `ignore`; omitted thresholds inherit global budgets. Role policy can override file bytes/lines, function ceilings, clone enablement/thresholds, and mutation evidence eligibility. A role cannot opt a non-source file into mutation evidence.

`file_size_severity`, `function_size_severity`, and `clone_severity` override
severity only for their finding category. Function size includes function lines
and statement counts; it does not include cyclomatic complexity,
nesting or parameters. Omitted category severities inherit the
role severity. A category override takes precedence even when `severity` is
explicitly set. These keys never change parser/read/classification or clone
index failure severity. Use category-specific `error` overrides when the
project needs test size or duplication to block.

`clone_enabled` is tri-state: `true` explicitly enables clone analysis for that role, `false` disables it, and an omitted key inherits `[clones].enabled`. Presets leave `source`, `test`, and `fixture` omitted so the global clone setting remains the master-like default; `generated` and `migration` are explicitly disabled.

## File and function budgets

Version 0.6.0 removes cognitive complexity, Halstead, ABC, and CRAP metrics. Remove
`max_cognitive`, `max_halstead_difficulty`, and `max_abc` from function budgets and role policies,
and `max_crap_score` from coverage policy. These obsolete keys are rejected as
unknown fields; they are never silently ignored. Coverage floors remain required
when enabled.

```toml
[budgets.files]
max_bytes = 32768

[budgets.files.max_lines]
rs = 499
ts = 400
tsx = 400
js = 400
jsx = 400
default = 350

[budgets.files.exclusions]
paths = ["src/generated/**"]

[budgets.functions]
max_cyclomatic = 10
max_parameters = 5
max_lines = 80
max_statements = 30
max_nesting_depth = 4
```

File limits use raw bytes and physical lines. Function limits come from Tree-sitter metrics for supported parser targets. `[budgets.files.exclusions].paths` skips only byte/line checks and emits an advisory; it does not suppress anti-gaming, invariants, parsing, clones, role classification, or generated freshness.

The unvalidated cognitive-complexity scorer is removed entirely. A future
replacement must be validated for Rust and JavaScript/TypeScript.

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

Rules are repository-owned, line-level checks for import strings, call spellings,
or tokens. `from` and `exclude` select repository paths; import globs match source
spellings, including lexical expansion of Rust grouped imports and a slash-form
alias of Rust paths. These aliases are not resolved files. This engine does not
build a dependency graph, resolve aliases/re-exports, evaluate Cargo features,
or prove reachability. Multi-line syntax is outside this line-rule contract.
Use configured specialist lint commands for dependency graphs or type-aware
architecture checks; their failures remain required orchestration evidence.

## Clone detection

```toml
[clones]
enabled = true
min_lines = 5
min_tokens = 50
excludes = ["tests/fixtures/**"]
```

Eligible source, test, and fixture files are analyzed in separate role groups using normalized lexical token streams and bounded rolling-hash windows. `excludes` belongs only to clone detection and emits an advisory when matching files are present. In `check --diff`, Git-changed/staged inventory is selected by default, explicit existing paths add to static/clone selection, and Hardgate indexes the full repository to retain clone pairs touching Git-changed/staged files or explicitly selected existing paths. Every current clone violation has a stable fingerprint over normalized token kinds; it excludes paths and physical line numbers, allowing rename lineage to preserve identity.

Detection and failure thresholds are separate:

| Role | strict-agent detection | balanced / legacy detection | Default failure policy |
| --- | --- | --- | --- |
| Source | 5 lines and 50 tokens | 8 lines and 80 tokens | At least 10 lines and 100 tokens (strict), 15 lines and 150 tokens (balanced/legacy) |
| Test | 8 lines and 80 tokens | 12 lines and 120 tokens | Advisory |
| Fixture | 20 lines and 200 tokens | 30 lines and 300 tokens | Advisory |

`roles.<role>.clone_block_min_lines` and `clone_block_min_tokens` are positive,
inclusive blocking minimums. A detected clone below either minimum becomes an
advisory containing both files, size, and fingerprint. Both configured minimums
must be met before an error-severity clone blocks. Omitted blocking minimums add
no restriction beyond detection; setting them below detection makes every
detected clone eligible to block. Warning/ignore clone severity remains
warning/ignore regardless of size. No clone input is removed by these minimums;
an incomplete clone index still follows the role's evidence-failure severity.

Custom policies retain severity inheritance without preset category overrides.
To retain the earlier all-detected-clones-block behavior under strict-agent,
set source blocking minimums to 5/50; for test enforcement, explicitly set its
three category severities to `error`. Existing explicit numeric overrides
remain authoritative. Review `hardgate config` after upgrading a preset-based
policy because omitted fields inherit the new defaults.

## Generated-artifact freshness

```toml
[generated]
enabled = true
freshness_command = "sh -c 'pnpm generate && git diff --exit-code -- generated/'"
timeout_secs = 300
```

When enabled, `freshness_command` is required and runs in `check` (including `--diff`). A missing command, timeout, non-zero exit, or runner failure is blocking current evidence. Freshness has its own timeout and is independent of `[budgets.files.exclusions]`; excluding generated files from a size check never disables freshness. Freshness is not part of the legacy static ratchet.

Configured commands are quote-aware tokenized arguments launched directly; Hardgate does not invoke an implicit shell. Shell operators such as `&&`, pipes, and redirection are ordinary arguments unless the command explicitly invokes a shell, for example `sh -c 'command-a && command-b'`.

## Legacy reference and ratchet

```toml
[legacy]
reference_branch = "origin/main"
ratchet = true
```

`ratchet = true` requires a non-empty reference. Hardgate resolves the Git merge base, loads the baseline snapshot, and analyzes baseline static findings. Existing non-worsened static debt can be grandfathered as advisories; new or worsened findings with effective role severity `error` remain blocking, `warning` findings remain advisories, and `ignore` findings are omitted. Pure rename lineage maps the current path back to the baseline path. Stable clone fingerprints remove line-number dependence. Retained findings are annotated with changed files or changed hunk ranges.

The ratchet applies only to static. Coverage, mutation, generated freshness, and orchestration are evaluated against the current tree and remain blocking; they are never grandfathered. If the reference, merge base, snapshot, or baseline analysis cannot be loaded, the ratchet reports a blocking evidence failure.

With the ratchet enabled, `check --diff` still uses actual Git-changed
executable lines for LCOV, while static and clone analysis disables diff
filtering but honors explicit existing paths added to the selected static/clone
scope: the selected scope is the full current tree when no paths are supplied.
The ratchet still loads and validates the full configured reference snapshot,
then compares it only to selected current static findings. Without a
ratchet, Git-changed/staged inventory is the default and explicit existing
paths add to static/clone selection.

## Coverage evidence

```toml
[coverage]
enabled = true
report = "coverage/lcov.info"
min_line_percent = 95.0
min_function_percent = 95.0
min_branch_percent = 90.0
critical_paths = ["src/core.ts"]
```

Only LCOV is parsed. Full checks evaluate global line/function/branch floors, critical paths, and missing source records. Source-role Rust files with no parsed executable functions (for example, declaration-only module files) do not require an LCOV source record; every Rust source with a parsed function and every non-Rust Source file remains required. `check --diff` filters Git changes to actual changed executable lines in AST-supported source-role files and reports uncovered lines or missing file records. `check` resolves the report as follows: an explicit CLI path takes precedence over `coverage.report`; it does not auto-discover conventional report filenames. A missing path, empty, unreadable, or malformed report is blocking whenever coverage is enabled, regardless of `gate.strict`.

When filtering inline Rust tests, overlapping LLVM line summaries are scored
conservatively: unassigned summary observations remain in the denominator and
only proven production line details receive hit credit. Function and branch
details must still establish an unambiguous production/test split.

This repository's self-gate generates branch LCOV with the pinned
`RUST_COVERAGE_TOOLCHAIN` (`nightly-2026-09-04`) because Rust branch
instrumentation is unstable. The producer-only nightly toolchain does not
change the Rust 1.90 MSRV or the Rust 1.98.1 pin used for normal build/test
gates. The helper includes executable `build.rs` in that LCOV report.

`check` accepts optional path arguments for the current static
inventory and coverage source matching only. Mutation-report ingestion and
generated freshness remain configured/full. The ratchet still loads and
validates the full configured reference snapshot, then compares it only to the
selected current static findings; explicit paths do not widen that
current selection.

## Mutation report evidence

```toml
[mutation]
enabled = true
min_score = 85.0
reports = ["reports/stryker-mutation.json"]
```

`check` evaluate source-bound Stryker (`files`) or cargo-mutants (`outcomes`) JSON. Raw generic outcome-count scoring remains a low-level library API. Empty reports, empty outcome arrays, missing reports, parse errors, and reports with no viable outcomes are blocking when mutation is enabled. Scores use killed divided by killed plus survived. Timeout, compile-error, runner-error, and unviable outcomes are integrity findings and remain blocking; mutation timeout handling is not a user-weakenable exception.

Native execution and `hardgate mutate` were removed in 0.6.0. The old
`mutation.test_cmd`, `mutation.timeout_secs` and `mutation.max_mutants` settings
are rejected. Configure execution in cargo-mutants or Stryker; retain
`mutation.enabled`, `min_score` and `reports` for required evidence.

## Orchestration

```toml
[orchestration]
format_check = "cargo fmt --all -- --check"
format = "cargo fmt --all"
lint = "cargo clippy --workspace --all-targets --all-features --message-format=json -- -D warnings"
test_cmd = "cargo test --workspace --all-targets --locked"
additional_tests = ["cargo test --workspace --doc --locked"]
# Explicit project feature contract, for example:
feature_checks = ["cargo check --workspace --no-default-features --locked"]
# JS/TS: typecheck = "npm run typecheck"
timeout_secs = 300
```

`check` requires formatting verification and linting, plus configured tests, `additional_tests`, `typecheck`, and `feature_checks`. An omitted command is conservatively detected; unresolved format/lint setup blocks acceptance. All checks execute in a protected disposable copy. `fmt --check` uses the same read-only path; explicit `fmt` uses `format`, falling back to `format_check`. See [check selection and execution](CLI_AND_INTEGRATION.md#selecting-checks-and-interpreting-acceptance).

Generic dead-code analysis was removed in 0.6.0. `[analysis]` (including
`[analysis.dead_code]`) and `check --dead-code` are rejected. Use compiler and
linter diagnostics for target-aware analysis.

## Validation and fail-closed rules

Serde handles types and enum values; semantic validation rejects non-positive thresholds, invalid/duplicate globs (including invariant import globs), enabled freshness without a command, enabled legacy ratchet without a reference, and unsafe mutation settings. Empty required reports/outcomes, unreadable files, parser failures, Git failures, and configured command failures are never silently converted into a pass. The CLI retains an advisory when source discovery is empty and still evaluates enabled evidence; MCP `hardgate_check` rejects empty scopes/discovery explicitly.

### Check outputs and bound coverage

Ordinary verification runs in a disposable copy. Recognized cache records
(`.ruff_cache`, `.import_linter_cache`, `.pytest_cache`, `.eslintcache`, and
`__pycache__/*.pyc`) may change; other ignored source/test/config files remain
protected. An explicit classification rule protects a cache-named required input.
External virtualenv interpreter links must match the `pyvenv.cfg` runtime;
other external links are rejected. No Python analysis is provided.

A declared `coverage.report` ending in `.lcov` or `.info` is a disposable output
only when classified as unknown, generated or vendor. It can be generated
by `orchestration.test_cmd` in the copy, but cannot establish evidence freshness
or produce a receipt. Generate trusted evidence separately:

```sh
hardgate evidence vitest
# Rust branch coverage requires an installed branch-capable toolchain:
hardgate evidence cargo-llvm-cov --toolchain <installed-nightly>
hardgate check --format agent
```

Set `coverage.report = ".hardgate/evidence/coverage.lcov"` to use that producer
artifact. The original report and receipt are checked for source binding,
freshness and integrity regardless of disposable test output. Coverage tools
must not overwrite source/test/config inputs. Commands needing temporary files
should use their runtime `$TMPDIR`, for example
`mktemp -d "${TMPDIR:-/tmp}/finance-hardgate.XXXXXXXX"`.
