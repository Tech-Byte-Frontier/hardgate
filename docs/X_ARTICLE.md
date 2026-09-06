# Hardgate: an evidence contract for agent-assisted code

Coding agents are good at producing a plausible patch. They are also good at finding the shortest route to a green command: a suppression pragma, a copied block, an unscanned generated file, or a report that no longer describes the current tree.

Hardgate makes the acceptance rule explicit and local. It inventories files, classifies repository roles, applies structural budgets and anti-gaming checks, evaluates configured evidence, and emits one report for a maintainer or an agent.

The important distinction is not “AI versus humans.” It is probabilistic generation versus deterministic policy:

```text
agent proposal -> repository policy -> evidence-backed report
```

The policy is role-aware. Source, test, generated, fixture, and migration roles have independent severity, budget, and clone policies. Mutation report validation preserves source-role ownership; specialist runners provide execution and mutation operators. Generated files can be excluded from handwritten debt checks without disabling a separate freshness command. A file-budget exclusion therefore cannot quietly turn off generated-artifact verification.

The command boundaries are deliberate:

- `hardgate check --diff` selects Git-changed/staged inventory by default, adds explicit existing paths to static/clone selection, compares clones against a full repository index, and scores only actual changed executable LCOV lines; with a legacy ratchet, static and clone comparison uses the full current selected scope (the whole tree when no paths are supplied);
- `hardgate check` includes formatting, linting, configured tests/type checks and enabled evidence;
- `hardgate check --checks policy` selects an explicit partial policy/evidence run;
- `hardgate evidence` invokes supported local coverage or mutation producers, records source/test/config identity, and verifies restoration before publishing evidence.

Enabled evidence is not optional by accident. Empty, missing, unreadable, or malformed reports fail closed. Disabled policies do not read stale report files. A configured legacy reference resolves a Git merge base and can grandfather existing non-worsened static debt while keeping new or worsened findings with effective role severity `error` blocking; `warning` findings remain advisories and `ignore` findings are omitted. Coverage, mutation, freshness, and configured orchestration remain current blocking evidence whenever their checks run and are never ratcheted. Stable clone fingerprints and rename lineage preserve safe identities without depending on physical line numbers.

For Rust, optional cargo-mutants execution first requires the complete Cargo
workspace baseline, including doctests, and preserves declared feature checks.
For JavaScript and TypeScript, optional StrykerJS execution uses the project's
local runner and configuration. Hardgate owns receipt validation and restoration;
the specialist owns mutation operators and test selection. Missing tools or
ambiguous configuration are setup failures, and partial selections remain explicit.

Agents can consume `--format agent` or JSON. MCP is stdio-only and intentionally static-only: `hardgate_check(paths?, diff?)`, `hardgate_scan_file(path)`, and `hardgate_get_metrics(path, symbol)`. `hardgate_check` and `hardgate_scan_file` use static reports; `diff` defaults to Git-changed/staged inventory, explicit existing paths add to static/clone selection, and clone matching uses the full repository index, while MCP never runs coverage. Invalid arguments/configuration, missing paths, empty scopes/discovery, and Git failures are outer tool errors. Read/parse failures remain report-level Hardgate `Failed` findings, with effective role severity `error` failing, `warning` advising, and `ignore` omitting the finding. For `hardgate_scan_file`, a read failure is an outer tool error while parse/static findings remain in its per-file report; `hardgate_get_metrics` reports read or missing-symbol errors.

Hardgate complements language linters, formatters, coverage providers, mutation runners, clone tools, and hosted products such as Qlty Cloud. Those tools own their language semantics, execution, or history. Hardgate owns the repository's local policy and the evidence boundary that says what was actually checked.

The 0.6.0 release contract covers Linux x64 GNU through Cargo, direct release
archives, npm, and pnpm. The thin launcher selects one native package. Release
verification retains signed tags, checksums, provenance, exact source identity,
and actual installed `hardgate check` behavior. The shell installer and other
platform builds are removed from future releases; existing artifacts are preserved.
