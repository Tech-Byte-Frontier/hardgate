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
Rust 1.98.1 crate MSRV and normal build/test gates; the helper includes the
executable `build.rs` in that LCOV report.

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

Metadata detection covers Rust, JavaScript/TypeScript, Python and Go, without
installing or executing project tools. Root manifests and existing scripts are
preferred. On POSIX hosts, root Python and JavaScript projects can combine
commands when both ecosystems supply the corresponding tool; these pairs run
through `sh`, and either failure fails the step. Missing pairs remain
unconfigured with a setup notice. Other mixed ecosystems, conflicting package
managers and nested-only packages require explicit commands or initialization
within the package. JS
config-based tool fallbacks require repository-local executables. Python tools
must be installed in the invoking environment; Go format checks require POSIX
`sh`. No preset assigns JavaScript tools to every ecosystem.

`init` defaults to balanced and leaves existing policies untouched. No-config
execution still defaults to strict-agent; adoption is an explicit policy choice.
Strict-agent requires real LCOV and mutation reports
for both `check` and `verify`; start with `hardgate config` to inspect missing
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

### Reference context and dead-code limits

Dead-code analysis uses all discoverable source, test, generated and fixture
references even for `--diff` or explicit paths. Scope filters the reported
findings, so an unchanged importer can keep a changed export live. A required
reference that cannot be read produces an incomplete-context failure.

The analyzer indexes words and recognizes common import/module declarations;
it does not implement compiler module resolution or prove runtime reachability.
Comments, strings, unrelated same-named symbols and same-stem modules may keep
otherwise unused code live. Dynamic imports, reflection and language-specific
module resolution remain heuristic boundaries. Confirm a finding before
removing code, especially public library exports.

## `hardgate check`

`check` runs static engines and every enabled report/freshness evaluator:

- role-aware file bytes/physical lines and Tree-sitter function budgets;
- suppression and custom-token checks;
- declarative import/call/token invariants;
- bounded token-stream clone detection;
- configured dead-code analysis when enabled (or requested with `--dead-code`);
- LCOV coverage and mutation-report evaluation when those policies are enabled;
- generated-artifact freshness when `[generated].enabled = true`.

```sh
hardgate check
hardgate check --dead-code
hardgate check --coverage-report coverage/lcov.info
hardgate check --format agent
hardgate check --format json
hardgate check --compact
hardgate check --summary
hardgate check src/routes/revenue.ts
```

`--coverage-report` only supplies a path; coverage still has to be enabled. Likewise, mutation report paths are read only when mutation is enabled. Empty or missing required reports, malformed reports, missing source records, and failing generated freshness commands are blocking evidence failures. A disabled engine does not inspect a stale report that happens to exist.

If discovery finds no files, the CLI emits an empty-discovery advisory and continues through every enabled report, freshness, and legacy step. It does not treat the advisory itself as a violation.

### `check --diff`

```sh
hardgate check --diff
hardgate check --diff src/routes/revenue.ts
```

Git status and diff evidence select changed or staged inventory files by default, including untracked inventory files. Explicit existing paths add to static/clone selection. A missing Git worktree or malformed Git evidence fails closed. Static findings are scoped to the Git selection when no legacy ratchet is enabled.

In ordinary diff mode, clone analysis is different: it builds a full repository index of eligible role groups, then reports only clone pairs touching Git-changed/staged files or explicitly selected existing paths. This catches a new copy against an unchanged file. Clone fingerprints are content-only and line-independent, so the legacy matcher can preserve identity across a safe rename.

When `[legacy].ratchet = true`, static and clone analysis disables diff filtering but still honors explicit path filters: it uses the full current selected scope (the whole tree when no paths are supplied) to compare against the configured reference merge-base, even though ordinary `--diff` static mode selects changed/staged files by default. The ratchet still loads and validates the full configured reference snapshot, then compares it only to the selected current static/dead-code findings; explicit paths never widen that current selection. Existing non-worsened static findings (and configured dead-code findings) may be grandfathered as advisories; new or worsened findings with effective role severity `error` remain blocking, `warning` findings remain advisories, and `ignore` findings are omitted. Retained findings are annotated with changed-file or changed-hunk context. Enabled coverage is evaluated only on actual changed executable lines from AST-supported source-role files. Mutation reports and generated freshness remain current blocking evidence; orchestration still requires `--all`.

## `hardgate check --all`

`--all` adds the configured `[orchestration]` format-check, lint, and test commands to `check`. Commands run sequentially from the repository root with a repository-local `node_modules/.bin` available on `PATH`. Hardgate does not discover commands, install tools, or run native mutation as part of `--all`.

```sh
hardgate check --all --format agent
```

An absent command is skipped because it was not configured; a configured command that is empty, unavailable, times out, or exits non-zero is an orchestration finding.

## `hardgate verify`

`verify` runs full-tree static analysis, configured dead-code analysis and the evidence gate by
default. Optional path arguments scope the current static/dead-code inventory
and coverage source matching only; mutation-report ingestion and generated
freshness continue to use their configured/full scope. The ratchet still loads
and validates the full configured reference snapshot, then compares it only to
the selected current static/dead-code findings; explicit paths do not widen that
current selection:

```sh
hardgate verify
hardgate verify --coverage-report coverage/lcov.info \
  --mutation-report reports/stryker-mutation.json
hardgate verify --format agent
hardgate verify --format json --summary
hardgate verify packages/backend
```

It runs static analysis, enabled coverage/mutation reports, generated freshness,
and the configured legacy static/dead-code ratchet. It does not run
formatter/linter/test orchestration, Stryker, cargo-mutants, or native AST
mutation. Explicitly enabled report/freshness/reference failures are blocking
regardless of `gate.strict`; that flag controls static/classification evidence
fallback. Empty reports and reports with no recognized outcomes fail closed.

Accepted report inputs are LCOV for coverage and Stryker-shaped, cargo-mutants-shaped, or generic outcome-count JSON for mutation. Mutation scores count killed versus survived; timeout, compile-error, runner-error, and unviable outcomes are integrity findings.

## `hardgate mutate`

Run native AST mutation testing against classified source-role files when the
mutation policy is enabled:

```sh
hardgate mutate --diff
hardgate mutate --scoped src/services/auth.ts --timeout 5 --max-mutants 20
hardgate mutate --scoped src/services/auth.ts \
  --test-cmd 'pnpm test {file}' --format agent
hardgate mutate --json
```

If `[mutation].enabled = false`, `mutate` prints a disabled-policy note and
exits successfully without discovering targets, running a baseline, or
executing mutants. The target and no-target rules below apply only when native
mutation is enabled.

The native runner:

1. selects supported production (`source`) files, never tests or generated/fixture files;
2. copies current workspace inputs to a private temporary directory and resolves test commands there;
3. executes an unmutated baseline and stops before mutants if that baseline fails;
4. applies bounded binary/boolean AST mutations one at a time;
5. records killed, survived, timeout, compile-error, runner-error, equivalent, and unviable outcomes;
6. restores and verifies the copied source bytes after every mutant, then removes the temporary workspace.

The CLI copies dirty, untracked, and ignored regular files, including installed
dependencies, without hardlinking them to live inputs. `.git` administrative
data and directories named `target` are omitted. Test commands run from the
copied repository or resolved package root; Cargo uses a fresh `target` inside
that copy even when the invoking environment sets `CARGO_TARGET_DIR`.
Commands requiring Git administrative data must be adapted before the
unmutated baseline can pass. Internal symlinks are remapped into the copy;
external symlinks, special files, and hardlinked mutation targets fail before
tests. Choose a workspace root containing the required sources/dependencies.

SIGINT/SIGTERM stop and reap owned test processes, verify restoration, and
remove the copy before exit 130/143. Later edits in the original workspace are
preserved. SIGKILL cannot run cleanup: the original source remains untouched,
but a private `hardgate-mutation-<pid>-<id>` directory and test processes may
remain. Stop those processes before deleting that exact temporary directory.
Use an external `TMPDIR` with enough space for copied inputs and a fresh build.
Mutation holds a per-user slot across projects, caps common worker defaults, and
checks Linux memory pressure. Eligible Linux hosts also apply aggregate scope
limits; macOS has no equivalent memory telemetry or aggregate cap. See
[native mutation resources](MUTATION_RESOURCES.md) for thresholds and limitations.
Resource aborts provide no mutation credit. Configured commands are trusted project code: the
copy is not an operating-system sandbox for explicit absolute-path writes or
external services. The low-level library runner still operates on its supplied
root; library callers should supply their own disposable workspace.

A scope with no viable mutation points fails. Native mutation is independent of mutation-report ingestion and does not invoke Stryker or cargo-mutants.

After an explicit scope is validated, any `mutate --diff` invocation, including
one with `--scoped`, is an explicitly reported no-op when no changed production
source exists. Missing, invalid, unsupported, or non-source explicit scopes
fail closed. Only a non-diff unrestricted invocation or explicit scope with no
eligible source-role target fails before mutation execution.

Native mutation is compiled for Linux and macOS targets, including all six
prebuilt/npm release binaries. Source builds targeting another operating
system fail closed before baseline or source writes because the required
process-group cleanup and atomic source-restoration guarantees are unavailable.
Static `check` and `scan` remain separate commands.

### JavaScript/TypeScript command resolution

For JavaScript-family targets (`.js`, `.jsx`, `.mjs`, `.cjs`, `.ts`, `.tsx`, `.mts`, `.cts`), Hardgate walks from the source directory toward the repository root.

1. Every existing ancestor `package.json`, including the nearest manifest, is parsed and validated. A malformed or unreadable manifest fails automatic resolution rather than falling back to an ancestor; pass `--test-cmd` to supply an explicit command.
2. A workspace root is recognized only from a validated declaration: a non-empty `workspaces` array/object in `package.json` or a valid `pnpm-workspace.yaml` `packages` list. Lockfiles and manager configuration files are package-manager hints only; they never prove workspace membership. Package-manager precedence remains `packageManager` in the nearest manifest, then the nearest lock/config hint, then npm as the fallback. Supported managers are npm, pnpm, Yarn, and Bun.
3. A local `test` script takes precedence. If it is absent, exactly one `test:*` script may be selected; multiple `test:*` scripts are ambiguous and fail closed, so use `--test-cmd`. Without a local script, one reliable child-local manifest, framework-config, or script signal supplies a direct framework command and takes precedence over a workspace-root script. Framework selection uses only validated manifest fields, known config filenames, and unambiguous script commands; it does not scan dependency packages.
4. Only when the child has neither a local script nor a reliable local manifest/config/script signal may a validated enclosing workspace-root manifest supply a test script; that fallback runs from the workspace root. When a selected script names an unambiguous Jest, Vitest, or Playwright executable, that script supplies selector behavior. Composed or unrecognized scripts run their script command without an inferred selector. Malformed or multiple framework hints fall back to a full-suite command, or use `--test-cmd`.
5. A matching `<stem>.test.<ext>` or `<stem>.spec.<ext>` is searched beside the source, under `__tests__`/`tests`, and in nested test roots (bounded depth). A child script runs from its package root; a workspace fallback script runs from the workspace root; a framework-only command runs from its config root (or the package root for a manifest-only hint); and the supplied repository root is the final fallback. If no reliable match exists, the full suite is selected.

The generated command uses the detected manager's local binary: `npm test`/`npm run`, `pnpm test`/`pnpm run`, `yarn test`/`yarn <script>`, or `bun test`/`bun run`; direct framework fallback uses `npm exec --offline`, `pnpm exec`, `yarn exec`, or `bun x --no-install`. Jest receives its normal file selector, Vitest receives `run`, and Playwright receives `test` when selector inference is valid. A project-specific `--test-cmd` is the authoritative override. No resolver path downloads packages; unavailable managers, binaries, malformed manifests, ambiguous scripts, and other resolver failures are baseline failures.

## Saved reports

Inspect a full saved gate report without loading the current policy or rescanning:

```sh
hardgate check --json --output gate.json
hardgate report gate.json --engine complexity --top 5 --json
hardgate report gate.json --metric 'Cognitive Complexity'
hardgate report compare before.json after.json --json --output comparison.json
```

Inspection filters the displayed findings and preserves the saved verdict and
exit status, including missing-evidence failures. JSON `inspection` metadata
distinguishes the original error total from the displayed count. Unknown engine
names fail instead of silently ignoring the filter. `--metric` selects matching
metric categories; `--top` selects findings with file locations, leaving tool
failure status in the saved verdict.

Comparison accepts terminal or JSON output and returns the after-report's exit
status. It lists added, removed and retained findings. Missing execution metadata,
changed engine selection, policy, roots or scope, incomplete evidence, and
filtered views prevent a claim of equivalent evaluation scope. Diff reports lack
a complete resolved inventory and are also marked non-equivalent. Removed findings
are not proof of remediation when the evaluation scope differs.

`--output PATH` saves the final rendered report atomically for `check`, `scan`,
`verify`, `mutate` and saved-report commands while retaining stdout output.
Mutation no-ops are saved too; setup errors use the normal error channel.
`mutate --summary` or `--format summary` suppresses per-mutant progress and shows
the score, outcomes, survivors and restoration status. `mutate --json --summary`
also retains execution metadata. `check --progress jsonl` emits stage events to
stderr; the final report remains the authority for engine completion and verdict.

## `hardgate scan <file>`

Inspect one existing file using role-aware safety and AST metrics:

```sh
hardgate scan src/services/auth.ts
hardgate scan --format json --summary src/services/auth.ts
```

Unsupported inventory formats can still receive applicable file/safety checks but do not produce function metrics. Missing or unreadable paths fail closed.

Scan includes every analyzed function, including those within budget. Full JSON
exposes `functions` with locations, cyclomatic/cognitive complexity, parameters,
size, nesting, statements, Halstead difficulty and ABC score. Human formats show
the same measurements; summary JSON keeps its smaller aggregate shape.

## `hardgate fmt`

```sh
hardgate fmt
hardgate fmt --check
```

`fmt --check` runs `[orchestration].format_check`; `fmt` runs `format`, falling back to `format_check` when no write command is configured. Commands run from the repository root with local Node binaries available. A configured command failure is blocking for this command.

An unconfigured formatter is a setup failure with exit 2.

## Output modes

`check`, `scan`, and `verify` accept `--format terminal|agent|json|compact|summary`, plus `--json`, `--compact`/`--no-snippets`, and `--summary`. `mutate` accepts terminal, agent, or JSON output. JSON is a single machine-readable report; agent output is structured Markdown with actionable locations.

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
one per-user workload slot. Help, version and shell completion generation do not
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

`hardgate_check` is fail-closed for outer tool errors: invalid arguments/configuration, missing paths, empty path arrays, empty discovery, and Git failures return an explicit failed response. Read/parse failures remain report-level Hardgate `Failed` findings, with effective role severity `error` failing the report, `warning` producing an advisory, and `ignore` omitting the finding. It never runs coverage/mutation reports, generated freshness, dead-code analysis, orchestration, or native mutation. The static report uses the same engine path as the CLI; optional `diff` selects Git-changed/staged scope by default, explicit existing paths add to static/clone selection, and clone matching uses the full repository index. MCP never runs coverage. For `hardgate_scan_file`, a read failure is an outer tool error while parse/static findings remain in its per-file report; `hardgate_get_metrics` reports read or missing-symbol errors explicitly.

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
For a full evidence gate, run: hardgate verify --format agent
For configured formatter/linter/test commands, run: hardgate check --all --format agent
For native mutation proof, run: hardgate mutate --format agent
```
