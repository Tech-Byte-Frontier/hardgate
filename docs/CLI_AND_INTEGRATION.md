# CLI reference and agent integration

Hardgate searches upward from the invocation directory for the nearest
`hardgate.toml`, stopping at the first Git repository boundary (including a
worktree `.git` file) or the filesystem root. That policy's directory is the
configuration root. With no policy, strict-agent defaults apply at the Git
root, or at the invocation directory outside Git. A blocking finding produces
a nonzero exit; advisories remain visible context.

`--config FILE` selects an explicit policy instead. A missing, unreadable, or
invalid explicit policy fails; it never silently selects defaults. Unknown
keys in all fixed configuration tables fail with valid field names, while
intentional aliases and dynamic extension budgets remain supported.

Policy globs, classification, report paths, generated freshness, and
orchestrated commands use the configuration root. CLI paths (including report
overrides) remain relative to the invocation directory. Full checks cover the
configuration root even from a nested directory; pass `.` to select the
invocation directory. Nested monorepo policies take precedence over parent
policies; use `--config ../hardgate.toml` to select a parent explicitly.

Inspect the complete merged, validated policy without executing tools:

```sh
hardgate config
hardgate --config policy.toml config --format json
```

TOML inspection includes root/policy comments and round-trips as an effective
policy. JSON includes `schema_version`, `root`, `config_path`,
`invocation_dir`, and `effective`. MCP uses the same discovery and explicit
`--config` authority; tool paths remain relative to its launch directory.

The repository's self-gate generates branch LCOV with the pinned
`RUST_COVERAGE_TOOLCHAIN` (`nightly-2026-09-04`) because Rust branch
instrumentation is unstable. This producer-only toolchain is separate from the
Rust 1.90 MSRV and the Rust 1.98.1 pin used for normal build/test gates.
The helper includes executable `build.rs` in that LCOV report.

## `hardgate init`

Write a commented configuration template without overwriting an existing file:

```sh
hardgate init --preset strict-agent
hardgate init --preset balanced
hardgate init --preset legacy-migration
hardgate init --preset custom
```

The default file contains a commented preset and project overrides. `--full`
expands the effective policy; `--preview` prints valid TOML to stdout, with the
setup summary on stderr, without writing a file. Existing files, directories and
symlinks are preserved. `--config` selects existing policy and cannot be used
with init.

```sh
hardgate init --preset balanced --preview
hardgate init --preset strict-agent --full
hardgate init --preset balanced --format-check 'pnpm run format:check' \
  --format-command 'pnpm run format' --lint 'pnpm run lint'
```

Metadata detection covers Rust and JavaScript/TypeScript without installing or
executing project tools. Root manifests and explicitly configured tools are preferred.
Mixed Rust/JS repositories, conflicting package managers and nested-only
packages require explicit commands or initialization within the package.
Formatter and linter selection is independent. Recognized whole-project JS scripts
are converted to direct verification commands, including scripts that request fixes.
Custom script paths/options require an explicit command; Hardgate reports uncertainty
instead of guessing scope. Tool configurations and declared dependencies take priority;
otherwise JS defaults to Oxfmt and Oxlint. Multiple configured tools for one role require
an explicit choice. Automatically detected configuration tools require repository-local
executables; Hardgate never installs them. Explicit command overrides remain the
repository owner's responsibility and should perform verification without writes.

`init` defaults to balanced and leaves existing policies untouched. No-config
execution still defaults to strict-agent; adoption is an explicit policy choice.
Strict-agent requires real LCOV and mutation reports
for `check`; start with `hardgate config` to inspect missing
setup. Balanced is a structural starting point with those evidence engines
disabled. Custom uses ordinary defaults, including clone and safety policies.
Legacy-migration enables the static ratchet and requires a resolvable trusted
reference with a merge-base; init checks `origin/main` but does not fetch it.

Preset test size and duplication findings, and source clones below the blocking
minimum, are advisories. They remain in report output without contributing to
blocking violation totals. Test complexity/safety and role evidence failures
remain blocking. See [category severity and preset budgets](CONFIGURATION_SPEC.md)
before interpreting a reduced violation total as code remediation. A legacy
ratchet verdict explicitly describes acceptance of new/worsened blocking debt
in its selected scope, not a debt-free repository.

## `hardgate check`

`check` is the combined acceptance command. It runs formatting verification and linting by default, configured tests/type checks, and every enabled policy/evidence evaluator:

- role-aware file bytes/physical lines and Tree-sitter function budgets;
- suppression and custom-token checks;
- declarative import/call/token invariants;
- bounded token-stream clone detection;
- LCOV coverage and mutation-report evaluation when those policies are enabled;
- generated-artifact freshness when `[generated].enabled = true`.

```sh
hardgate check
hardgate check --coverage-report .hardgate/evidence/coverage.lcov
hardgate check --format agent
hardgate check --format json
hardgate check --compact
hardgate check --engine clones --format agent --report-json gate.json
hardgate check --engine clones --snippets --max-diagnostics 5
hardgate check --summary
hardgate check src/routes/revenue.ts
```

Explicit `--coverage-report` and `--mutation-report` arguments enable and require that evidence for this invocation. Otherwise the configured enablement and paths apply. Missing, stale or malformed required evidence blocks acceptance. Report arguments require selection of the `policy` check group.

If discovery finds no files, the CLI emits an empty-discovery advisory and continues through every enabled report, freshness, and legacy step. It does not treat the advisory itself as a violation.

### `check --diff`

```sh
hardgate check --diff
hardgate check --diff src/routes/revenue.ts
```

Git status and diff evidence select changed or staged inventory files by default, including untracked inventory files. Explicit existing paths add to static/clone selection. A missing Git worktree or malformed Git evidence fails closed. Static findings are scoped to the Git selection when no legacy ratchet is enabled.

In ordinary diff mode, clone analysis is different: it builds a full repository index of eligible role groups, then reports only clone pairs touching Git-changed/staged files or explicitly selected existing paths. This catches a new copy against an unchanged file. Clone fingerprints are content-only and line-independent, so the legacy matcher can preserve identity across a safe rename.

When `[legacy].ratchet = true`, static and clone analysis disables diff filtering but still honors explicit path filters: it uses the full current selected scope (the whole tree when no paths are supplied) to compare against the configured reference merge-base, even though ordinary `--diff` static mode selects changed/staged files by default. The ratchet still loads and validates the full configured reference snapshot, then compares it only to the selected current static findings; explicit paths never widen that current selection. Existing non-worsened static findings may be grandfathered as advisories; new or worsened findings with effective role severity `error` remain blocking, `warning` findings remain advisories, and `ignore` findings are omitted. Retained findings are annotated with changed-file or changed-hunk context. Enabled coverage is evaluated only on actual changed executable lines from AST-supported source-role files. Mutation reports and generated freshness remain current blocking evidence; configured orchestration still runs, in its configured workspace scope.

### Selecting checks and interpreting acceptance

`--engine clones` filters **display only**, using the same engine names and aliases
as `report`. It does not select execution. Verdict, full category counts, scope,
omitted requirements, advisories and setup/tool failure context remain visible,
even with no matching findings or `--max-diagnostics 0`. `clones -> policy`:
`--checks clones` is invalid because clone detection belongs to the policy group.
Use `--checks policy --engine clones --compact` for explicitly partial policy triage.

`--checks policy,format,lint,tests,typecheck` selects named groups. Omit it to run all requirements. Path arguments and `--diff` narrow policy analysis; external commands retain the package/target/feature scope recorded in `execution.engines[].required_evidence`. A partial run can pass its requested checks and exit zero, but reports `partial: true`, `accepted: false`, and omitted requirements. Complete acceptance requires every selected engine to finish successfully. The former `verify` command and `--all` option are removed.

```sh
hardgate check --checks policy --json
hardgate check --checks format,lint
hardgate check --mutation-report .hardgate/evidence/mutation.json
```

Missing format/lint commands or executables are setup failures. Hardgate resolves commands using the same conservative project detection as `init`, preserving explicit commands and each formatter/linter choice independently. It does not install tools or generate evidence during `check`.

Rust defaults cover workspace members with `cargo test --workspace --all-targets --locked`, plus a separate `cargo test --workspace --doc --locked`. Declare incompatible or no-default-feature compile checks in `orchestration.feature_checks`; Cargo feature combinations are not guessed. Clippy runs across workspace targets and receives JSON output flags. Individual rustc/Clippy findings retain rule, source location, package and target, separately from execution failures. Truncated or unfinished diagnostic streams cannot establish acceptance.

### Read-only execution

Check commands run sequentially in an independent input copy, sharing its disposable Cargo target. Linux Landlock ABI 3 or newer must be enabled; without it, external checks fail with setup guidance. Child writes are restricted to that copy, a disjoint Cargo cache, and `/dev/null`. Temporary and JS cache output stays in the disposable copy. pnpm automatic dependency verification is disabled inside disposable runs: copied install metadata must not trigger an install or block an already installed verifier. Hardgate does not install dependencies. Missing tools and failed checks still fail, and Linux isolation keeps the original dependency tree unwritable when required. For older pnpm versions that mishandle this setting in nested scripts, configure a direct local verifier. Commands are parsed without a shell; use `sh -c 'first && second'` when shell operators are required. Original absolute source paths remain unwritable. Known cache records from Ruff, import-linter, pytest, ESLint and Python bytecode
may change in the disposable copy. Other files remain protected even when
Git ignores them; explicit classification rules can protect cache-named inputs.
Configured commands receive a disposable `UV_CACHE_DIR`, including when the
caller has set an external uv cache path.
Verified virtualenv interpreter links are copied as tool links bound to the
runtime declared by `pyvenv.cfg`; arbitrary external source/data links still fail.
Virtualenv contents remain copied and bound, without adding Python analysis.

Changes to copied source/test/config inputs fail the check; intentional fixes require `hardgate fmt` or direct tool invocation. The guard restricts file-content and directory-entry writes; it is not a general sandbox for arbitrary hostile programs or external services. Git administrative data is not copied, so commands requiring checkout metadata must report their unmet requirement. Receipts describe source freshness, not a hermetic environment.

A configured coverage report with a `.lcov` or `.info` name can be rewritten in
the disposable copy when its role is unknown, generated or vendor. This
does not publish that output or issue a receipt. Run a supported evidence producer
first (below), and point `coverage.report` at its bound report. Missing, stale and
tampered original evidence still fail even if the test command writes a report.

If a configured command hardcodes `/tmp`, containment can reject it. The diagnostic
prints the actual disposable runtime `$TMPDIR`; use
`mktemp -d "${TMPDIR:-/tmp}/finance-hardgate.XXXXXXXX"` inside that command.
A shell outside Hardgate has its own host/agent permissions and `$TMPDIR`.

## Mutation producers

Native mutation execution was removed in 0.6.0. Run cargo-mutants for Rust or
Stryker for JavaScript/TypeScript through `hardgate evidence`, and configure their bound JSON reports in
`[mutation].reports`. Hardgate evaluates the results; it does not generate
mutants or guess the project test command. Missing required reports, empty
outcomes and runner integrity failures remain blocking.

## Saved reports

Inspect a full saved gate report without loading the current policy or rescanning:

```sh
hardgate check --engine clones --format agent --snippets --report-json gate.json
hardgate check --json --output gate.json
hardgate report gate.json --engine complexity --top 5 --json
hardgate report gate.json --metric 'Statement Count'
hardgate report compare before.json after.json --json --output comparison.json
```

Inspection filters the displayed findings and preserves the saved verdict and
exit status, including missing-evidence failures. JSON `inspection` metadata
distinguishes the original error total from the displayed count. Unknown engine
names fail instead of silently ignoring the filter. `--metric` selects matching
metric categories; `--top N` ranks files by finding count (ties by path), retaining both sides of
selected clone pairs. `--max-diagnostics N` limits findings **after** engine,
metric and top-file filtering. Counts say how many findings are displayed and
omitted; the original totals and failure context remain available.

Comparison accepts terminal or JSON output and returns the after-report's exit
status. It lists added, removed and retained findings. Missing execution metadata,
changed engine selection, policy, roots or scope, incomplete evidence, and
filtered views prevent a claim of equivalent evaluation scope. Diff reports lack
a complete resolved inventory and are also marked non-equivalent. Removed findings
are not proof of remediation when the evaluation scope differs.

`check --report-json PATH` saves a complete schema-v1 JSON report atomically in
the same run, independent of display filters, limits or human format. Add
`--snippets` during capture to retain bounded excerpts for later inspection.
Saved inspection uses captured excerpts only; missing excerpts are reported,
never filled from changed live files. `--output` and `--report-json` must name
different files. Complete evidence can be inspected without rerunning tools:

```sh
# All requirements execute; display only the first five clones. Exit 1/2 still fails.
hardgate check --engine clones --compact --max-diagnostics 5 --report-json gate.json
hardgate report gate.json --engine clones --format agent
# Partial policy triage: exit 0 does not establish complete acceptance.
hardgate check --checks policy --engine clones --format agent
# Complete acceptance; produce any required bound evidence first.
hardgate check --format agent
# Bash/zsh pipelines must preserve Hardgate's exit status:
set -o pipefail
hardgate check --json | jq '.clone_violations'
```

`--output PATH` saves the final rendered report atomically for `check`, `scan`,
and saved-report commands while retaining stdout output.
Setup errors use the normal error channel. `check --progress jsonl` emits stage events to
stderr; the final report remains the authority for engine completion and verdict.
Running external commands emit a heartbeat every 10 seconds with active phase,
phase elapsed time, and timeout. Interactive terminals and explicit JSONL mode
also receive the initial event. Mutation heartbeats include the latest bounded
tool progress excerpt when available; silent tools do not have an invented
completion percentage. Progress stays on stderr so stdout reports remain valid.

## `hardgate scan <file>`

Inspect one existing file using role-aware safety and AST metrics:

```sh
hardgate scan src/services/auth.ts
hardgate scan --format json --summary src/services/auth.ts
```

Unsupported inventory formats can still receive applicable file/safety checks but do not produce function metrics. Missing or unreadable paths fail closed.

Parser rejections identify one-based line and character-column coordinates,
also present in JSON diagnostic locations. A Tree-sitter rejection does not
prove invalid source syntax: validate with the project compiler. If the compiler
accepts it, report a Hardgate parser limitation. For example,
`original<typeof import('react-dom/client')>()` can be expressed using an imported
type alias while parser support is incomplete. Rejected syntax remains missing
AST evidence and cannot silently pass the gate.

Scan includes every analyzed function, including those within budget. Full JSON
exposes `functions` with locations, cyclomatic complexity, parameters,
size, nesting and statements. Human formats show
the same measurements; summary JSON keeps its smaller aggregate shape.

## `hardgate doctor`

`hardgate doctor` performs a read-only preflight; `--json` produces a structured
report. It resolves configured and detected launchers (including local Node
binaries and direct package-manager `exec` targets), checks named `run` scripts
exist, reports missing format/lint setup, and verifies current evidence
receipts without executing project commands. Exit 2 means setup/evidence is
incomplete; exit 0 means these preflight checks passed, not project acceptance.
Launcher presence does not validate package-manager scripts, arguments, or
compiler versions. Missing evidence needs `hardgate evidence vitest`,
`hardgate evidence stryker`, `hardgate evidence cargo-mutants`, or
`hardgate evidence cargo-llvm-cov --toolchain <installed-nightly>` on Linux.
macOS supports native checks; evidence production requires Linux containment.
Linux preflight does not acquire an execution lease or certify runtime admission.

## `hardgate fmt`

```sh
hardgate fmt
hardgate fmt --check
hardgate fmt src/main.ts "src/file with spaces.ts"
hardgate fmt --changed
hardgate fmt --check --changed
```

`fmt --check` runs `[orchestration].format_check`; `fmt` runs `format`, falling back to `format_check` when no write command is configured. Commands run from the repository root with local Node binaries available. A configured command failure is blocking for this command.

An unconfigured formatter is an explicit unevaluated requirement (`format_check=incomplete`, exit 2), with “No formatter configured or detected” guidance. This does not indicate formatting violations. `--checks policy` remains available for partial structural triage; complete acceptance requires a formatter. Scoped formatting
requires an explicit file-aware template:

```toml
[orchestration]
format_files = "oxfmt {files}"
format_check_files = "oxfmt --check {files}"
```

The standalone `{files}` argument expands to individually quoted, sorted,
deduplicated file paths. It is not shell interpolation. Use a command that
honors file arguments; do not retain whole-repository arguments such as `.` or
`--all` in the scoped template. Package-manager scripts must forward their
arguments. Explicit paths resolve from the invocation directory, must be files
inside the configuration root, and cannot escape through symlinks. `--changed`
selects staged, unstaged, and untracked files under that root; deleted files
are skipped, and an empty selection runs no formatter. The whole-project
`format`/`format_check` commands remain separate from these templates.

## Output modes

`check` and `scan` accept `--format terminal|agent|json|compact|summary`, plus `--json`, `--compact`/`--no-snippets`, and `--summary`. JSON is a single machine-readable report. Agent output leads with verdict and
evaluated scope, then stable rule IDs, locations, measurements/limits and short
review guidance. Related function metrics share one location; clone pairs stay
together. Use `--details` for longer explanations and AST contributors, and
`--snippets` for bounded captured source excerpts.

Exit codes are **0** for success or an explicit no-op, **1** for policy
violations, and **2** when arguments, configuration, runtime failures or missing
required evidence prevent evaluation. Signal cancellation retains 130/143.
Closing stdout intentionally (for example, piping to `head`) exits 0 without a
panic. Native mutation stops and releases its isolated workspace if progress
output cannot be written. Command APIs return `CommandOutcome` instead of
terminating the calling process.

Argument and runtime errors requested with `--json`, `--format json` or
`--format=json` emit one JSON document on stdout and no duplicate stderr error.
All gate JSON, summary JSON, mutation/no-op and error documents include
`schema_version: 1`, `command`, `passed`, `status`, and `exit_code`. Reports also
carry the execution plan, config identity and individual engine states.
Runtime errors retain validated policy/scope when available; argument or invalid
policy errors have no execution plan. Help/version retain normal text output.
See the [machine-output contract](REPORT_SCHEMA.md) before migrating consumers.

`--max-diagnostics N` caps displayed findings, including `N=0`; analysis,
exit status and summary counts remain complete. `--snippets` adds excerpts from
the source bytes captured during analysis, including both sides of a clone.
Excerpts are limited to eight lines per location, 240 Unicode characters per
line and 64 KiB of snippet text in total. Summary output omits diagnostic details.
Stable [rule IDs](DIAGNOSTIC_RULES.md) identify findings independently of wording,
paths and line movement.

Analysis defaults to at most two workers, preserving a smaller Rayon setting.
`--threads N` selects a smaller positive count within that ceiling; it cannot
remove the OS resource boundary. Small source captures and AST batches run
sequentially below eight files. `--timing` adds total elapsed time to stderr.

Workload commands require verified CPU, memory, swap and task limits before
loading project input. On Linux, Hardgate establishes a systemd user scope when
it does not already inherit suitable cgroup-v2 limits. The scope covers analysis
and every child tool, including detached descendants. Separate invocations share
one per-user workload slot with maintenance runners. Each invocation uses a unique
scope identity; overlapping commands wait for the slot (up to 30 minutes), with
cancellation and contention reported separately from manager setup failures.
The scope leader retains the slot if its outer supervisor is terminated.
Help, version, shell completion generation, `init` and `config` do not
need a workload scope. Unsupported environments fail with exit 2 before work;
there is no implicit unrestricted fallback. See [resource limits](MUTATION_RESOURCES.md).

`--color auto|always|never` applies to human output. Explicit choices override
environment; auto honors `NO_COLOR`, then nonzero `CLICOLOR_FORCE`, then TTY,
`CLICOLOR=0` and `TERM=dumb`. JSON does not contain terminal styling.

## Shell completions

```sh
hardgate completions bash > hardgate.bash
hardgate completions zsh > _hardgate
hardgate completions fish > hardgate.fish
```

PowerShell and Elvish are also supported. Completion generation loads no policy
and runs no project tools. Source/install the generated script according to the
selected shell's completion setup.

## `hardgate mcp`

Launch the embedded Model Context Protocol server over standard input/output:

```sh
hardgate mcp
```

It accepts newline-delimited or `Content-Length`-framed JSON-RPC. The tool names are:

| Tool | Arguments | Scope |
| --- | --- | --- |
| `hardgate_check` | optional `paths: string[]`, optional `diff: boolean` | Static gate only, including role-aware checks and full-index clone behavior |
| `hardgate_scan_file` | required `path: string` | One-file safety and AST report |
| `hardgate_get_metrics` | required `path: string`, `symbol: string` | Metrics for one named function |

`hardgate_check` is fail-closed for outer tool errors: invalid arguments/configuration, missing paths, empty path arrays, empty discovery, and Git failures return an explicit failed response. Read/parse failures remain report-level Hardgate `Failed` findings, with effective role severity `error` failing the report, `warning` producing an advisory, and `ignore` omitting the finding. It never runs coverage/mutation reports, generated freshness, orchestration. The static report uses the same engine path as the CLI; optional `diff` selects Git-changed/staged scope by default, explicit existing paths add to static/clone selection, and clone matching uses the full repository index. MCP never runs coverage. For `hardgate_scan_file`, a read failure is an outer tool error while parse/static findings remain in its per-file report; `hardgate_get_metrics` reports read or missing-symbol errors explicitly.

Check and scan tools return their human report plus `structuredContent` using
the versioned report schema. An evaluated policy violation remains a report;
invalid tool requests retain the existing `isError` response contract.

Register the stdio server with an MCP-capable client:

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

For a hook or an agent instruction, describe the exact evidence requested:

```text
After editing, run: hardgate check --diff --format agent
For complete acceptance, run: hardgate check --format agent
Generate required mutation evidence with hardgate evidence cargo-mutants or stryker first.
```

## Source-bound evidence

`check` requires a producer receipt alongside each enabled coverage or
mutation report. A stale source/test/config snapshot, changed report, missing
receipt, unfinished runner, or unverified restoration fails acceptance. Configure
report paths before producing evidence; changing policy invalidates its receipt.

```sh
hardgate evidence cargo-llvm-cov --toolchain nightly-2026-09-04
hardgate evidence vitest
hardgate evidence cargo-mutants -- --package my-crate --re 'my_function'
hardgate evidence stryker
```

Artifacts default to `.hardgate/evidence/coverage.lcov` and
`.hardgate/evidence/mutation.json`, with adjacent `.hardgate.json` receipts.
`--name` permits separate feature/package samples. Rust producer arguments after
`--` select explicit packages, targets and features; the receipt records the exact
commands. Cargo mutation first runs `cargo test --workspace --locked`, preserving
requested feature/target flags and compatible runner configuration, before starting
mutants. A failing member or doctest blocks production. The receipt records the
successful prerequisite separately from the runner's own baseline; nextest is not
yet supported by this prerequisite integration. `--file`, `--re`, and `--shard`
select explicit mutation samples. Default LLVM production runs all workspace targets and doctests in two
steps, merging only those fresh profiles. JS scope comes from the project runner
configuration. Producers are optional, preinstalled tools; no dependency installation
occurs during generation. Validated producers: cargo-llvm-cov 0.9.0,
cargo-mutants 27.1.0, Vitest/V8 5.0.0 and StrykerJS 10.0.0.

Each producer runs in an independent input copy. Source/test/config bytes and
inventory must match before and after execution; Stryker's reported original
source must also match. Mutants must belong to configured source roles. Mutation
samples remain samples: a score does not prove all project code was mutated.
Receipts are local freshness records, not signatures or hermetic-build attestations;
installed dependency caches are represented by manifests/lockfiles. Low-level
report-scoring APIs remain separate from source-bound CLI acceptance.

The optional real ripgrep acceptance harness is
`scripts/check-ripgrep-mutation.mjs --repo RIPGREP --binary HARDGATE --output FRESH_DIRECTORY`.
It requires local cargo-mutants, cached dependencies, and the normal resource
boundary. It records a surviving `ByteSet::add` no-op, proves a new byte-insertion
assertion passes on original code, and proves that assertion kills the identical
mutant. Full workspace tests run for both baselines and mutant scenarios. Because
cargo-mutants 27.1.0 includes struct-field deletions despite the regex filter,
the harness retains the full listing and explicitly selects the requested mutant
by shard. This is one integration sample, not a repository-wide mutation score.
