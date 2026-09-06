# Machine output version 1

CLI gate reports, summary reports, mutation results, explicit mutation no-ops,
configuration inspection and errors use `schema_version: 1`. Gate command names
are `check` and `scan`; parser errors use `arguments` because
a valid command may not exist. MCP structured reports use `mcp_check` and
`mcp_scan`, preserving their narrower static scope.

Saved-report inspection and comparison use `command: "report"` and the same
version, verdict, status and exit-code fields. Inspection retains the original
saved verdict even when its filters hide every finding. Its `inspection` object
contains `filtered`, `original_total_errors` and `displayed_errors`; the category
arrays and view summary describe the filtered findings. Saving and inspecting a
filtered view again preserves an incomplete verdict. Comparison reports contain
`verdict_before`, `verdict_after`, finding differences and explicit scope/config
differences. `equivalent` requires known matching evaluation metadata and is not
proof that the source bytes are unchanged. Neither command rescans sources.

| Field | Contract |
| --- | --- |
| `passed` | Whether the evaluated policy has no blocking failure; never proof that every engine ran |
| `status` | `passed`, `violations`, `incomplete`, `noop`, or `error` |
| `exit_code` | 0 for pass/no-op, 1 for policy violations, 2 for inability to evaluate required evidence |
| `execution` | Command, scope, effective config identity and engine records; null when no validated plan exists |
| `summary` | Complete finding and scan counts, independent of display limits (gate reports) |
| `diagnostics` | Stable rule IDs, messages, locations, remediation, and optional captured source excerpts (full gate reports) |

Mutation no-ops preserve the existing `stage`, `kind`, and `message` fields.
Runtime and argument errors include these fields too. Normal mutation results
retain `stats`, scores and per-mutant outcomes. Configuration inspection retains
`effective`, `config_path`, `root`, and `invocation_dir`, and adds
`config_identity`; it does not run analysis engines.

## Execution evidence

`execution.scope.mode` is `repository`, `paths`, or `diff`. Paths are resolved
from the invocation directory; the config identity records the policy authority
and root. `policy_sha256` hashes effective policy serialized as canonical JSON
with recursively sorted object keys. Comments and whitespace do not change it;
overrides do. The hash identifies policy, not source freshness or a cached verdict.

Each engine has a stable `id`, policy `enabled`, command `selected`,
`required_evidence`, `state`, and optional `reason`. Evidence paths from policy
are relative to the configuration root; CLI report overrides are resolved paths.
Required-evidence descriptions identify inputs, not proof that those inputs exist.

| State | Meaning |
| --- | --- |
| `disabled` | Policy disables the engine or its external command is unconfigured |
| `skipped` | Enabled but outside command scope, or no eligible input was evaluated |
| `incomplete` | An input, parser, capacity, runner, or required report prevented complete evaluation |
| `failed` | Completed evaluation found blocking policy violations |
| `completed` | The selected engine evaluated eligible inputs successfully |
| `cached` | Reserved; current engines never emit a cached result |

Observations aggregate across files and role groups. Incomplete evidence is not
overwritten by a later successful group. An aborted command reports selected
engines as incomplete because complete evidence was not returned. In advisory
policy modes, an engine can be incomplete while `passed` remains true; its
advisory explains the gap. Engine reasons retain evidence failures even when
role severity `ignore` omits the individual finding. Do not use `passed` alone to infer required execution.
The legacy ratchet can grandfather prior static findings while retaining its
reference-evidence status.

`check` evaluates policy/evidence plus formatting, linting and configured tests/type checks. `--checks` selects explicit partial groups. `scan` evaluates one file's static metrics and safety policy; MCP remains static.

## Diagnostics and compatibility

Full reports retain the existing category violation arrays, `functions`,
advisories, top files and aggregate fields. New consumers should use
`diagnostics[].rule_id` from the [rule catalog](DIAGNOSTIC_RULES.md), not parse
mutable messages. The category order is budget, suppression, complexity,
invariant, clone, coverage, mutation, orchestration. Clone locations
include both ranges. Missing coordinates remain null; they are not invented.

In 0.6.0, function records contain size, parameter, statement, nesting,
cyclomatic metrics and their control-flow breakdowns. `test_only` identifies
functions analyzed under test policy, including colocated Rust tests and resolved
test-only helper modules. Original file locations are retained. The removed
metrics listed in the [configuration migration](CONFIGURATION_SPEC.md#file-and-function-budgets)
are no longer calculated, emitted, or enforced.

`--max-diagnostics N` limits the diagnostic stream and legacy category arrays
with one shared count. `total`, `shown`, and `omitted` describe that presentation;
`summary`, `passed`, engine states and exit status always use the complete result.
Summary JSON keeps its lean shape without individual diagnostics.

`--snippets` uses only captured source bytes. Eight lines per location, 240
Unicode characters per line and 64 KiB of total snippet text bound the output.
Control characters are escaped. Per-excerpt `truncated`, plus `snippet_bytes`
and `snippets_truncated`, describe retained text. No source excerpt is fabricated
when bytes are unavailable. These bounds do not truncate analysis or findings.

Version 1 adds fields to the earlier unversioned report shape and standardizes
exit codes. Consumers should accept unknown additive fields, check the version,
and distinguish exit 1 from exit 2. A major schema change requires a new version;
published rule IDs retain their meanings. Intentionally closed stdout exits 0
without a panic, so pipelines needing the gate verdict must keep stdout open
until the command finishes. Signal cancellation uses 130/143 outside the report.

## Combined acceptance and specialist findings

`partial`, `accepted`, and `omitted_requirements` distinguish a successful requested subset from complete repository acceptance. `passed` and the exit code describe the requested checks. Path/diff runs and omitted enabled engines make the result partial. `accepted` also requires every selected engine to be completed or a verified cache hit.

`tool_diagnostics` retains structured rustc/Clippy `tool`, `rule`, `level`, `message`, `file`, `line`, `column`, `end_line`, `package_id`, `target`, and `blocking`. Blocking findings contribute to `code_findings` and `specialist_findings`; `orchestration_violations` and `analysis_blockers` describe execution/evidence failures separately. Nonblocking compiler warnings remain available in the diagnostic records. Cargo artifact chatter is filtered before bounded capture; incomplete or truncated compiler diagnostics fail acceptance.

## Review targets and size

`review_targets` groups complexity findings by original function path, line,
column, end line, and name. `summary.function_review_targets` counts affected
functions; `total_errors` still counts all blocking findings. Display limits
bound the grouped metrics as well as category arrays and never change the full
summary or verdict. Columns distinguish anonymous functions sharing a line;
older saved reports without columns expose `null` in their review targets.

`file_sizes` records syntax-derived physical, code, documentation, comment, and
blank line counts per selected policy role. Function observations and findings
carry an optional `size` for the function's own span. Rust doc comments and
JSDoc-style blocks count as documentation; comment-like text inside strings does
not. Mixed lines can contribute to more than one category. Physical byte/line
budgets remain enforced as configured. Size observations guide review, and do
not prove that extracting code would improve the design. Older reports without
these observations remain readable without invented measurements.
