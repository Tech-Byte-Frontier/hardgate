# Hardgate: an evidence contract for agent-assisted code

Coding agents are good at producing a plausible patch. They are also good at finding the shortest route to a green command: a suppression pragma, a copied block, an unscanned generated file, or a report that no longer describes the current tree.

Hardgate makes the acceptance rule explicit and local. It inventories files, classifies repository roles, applies structural budgets and anti-gaming checks, evaluates configured evidence, and emits one report for a maintainer or an agent.

The important distinction is not “AI versus humans.” It is probabilistic generation versus deterministic policy:

```text
agent proposal -> repository policy -> evidence-backed report
```

The policy is role-aware. Source, test, generated, fixture, and migration roles have independent severity, budget, and clone policies. Native mutation is source-role-only; source mutation eligibility is configurable, while other roles must remain ineligible. Generated files can be excluded from handwritten debt checks without disabling a separate freshness command. A file-budget exclusion therefore cannot quietly turn off generated-artifact verification.

The command boundaries are deliberate:

- `hardgate check` runs static engines plus enabled coverage/mutation reports and freshness;
- `hardgate check --diff` selects Git-changed/staged inventory by default, adds explicit existing paths to static/clone selection, compares clones against a full repository index, and scores only actual changed executable LCOV lines; with a legacy ratchet, static and clone comparison uses the full current selected scope (the whole tree when no paths are supplied);
- `hardgate check --all` adds only formatter/linter/test commands configured by the repository;
- `hardgate verify` runs full static/dead-code and configured evidence, with optional paths narrowing only current static/dead-code inventory and coverage source matching; mutation reports and freshness remain configured/full, while the ratchet loads the full configured reference snapshot but compares it only to selected current static/dead-code findings without widening explicit paths;
- `hardgate mutate` runs a native unmutated baseline and AST mutants when enabled on source builds for Linux and macOS through the target-OS cfg, and prints a disabled-policy note before a successful no-op when disabled; after explicit scope validation, `--diff` (including `--scoped`) is also a successful no-op when no changed production source exists; missing, invalid, unsupported, or non-source explicit scopes fail closed; other operating systems fail closed before baseline or source writes. The prebuilt, npm, and shell-installer release contract remains exactly six x64/arm64 glibc/musl/macOS targets (Linux x64/arm64 glibc and musl, macOS x64/arm64), which does not constrain source builds.

Enabled evidence is not optional by accident. Empty, missing, unreadable, or malformed reports fail closed. Disabled policies do not read stale report files. A configured legacy reference resolves a Git merge base and can grandfather existing non-worsened static/dead-code debt while keeping new or worsened findings with effective role severity `error` blocking; `warning` findings remain advisories and `ignore` findings are omitted. Coverage, mutation, freshness, and configured orchestration remain current blocking evidence whenever their checks run and are never ratcheted. Stable clone fingerprints and rename lineage preserve safe identities without depending on physical line numbers.

For JavaScript and TypeScript, native mutation validates encountered package
manifests, recognizes only declared workspaces (lockfiles are manager hints),
detects npm, pnpm, Yarn, or Bun, and infers Jest, Vitest, or Playwright only
when selector behavior is unambiguous. A child test script wins; one `test:*`
script is allowed, and a reliable child-local manifest, framework-config, or
script signal wins over a validated enclosing workspace-root script. That root script
is used only with no local script or reliable local manifest/config/script signal; malformed manifests
or ambiguous scripts fail closed. It searches for a
matching test and falls back to the full suite. A project-specific
`--test-cmd` remains authoritative. The resolver uses local manager commands
and does not download packages at runtime. Framework selection uses only
validated manifest fields, known config filenames, and unambiguous script
commands; it does not scan dependency packages.

Agents can consume `--format agent` or JSON. MCP is stdio-only and intentionally static-only: `hardgate_check(paths?, diff?)`, `hardgate_scan_file(path)`, and `hardgate_get_metrics(path, symbol)`. `hardgate_check` and `hardgate_scan_file` use static reports; `diff` defaults to Git-changed/staged inventory, explicit existing paths add to static/clone selection, and clone matching uses the full repository index, while MCP never runs coverage. Invalid arguments/configuration, missing paths, empty scopes/discovery, and Git failures are outer tool errors. Read/parse failures remain report-level Hardgate `Failed` findings, with effective role severity `error` failing, `warning` advising, and `ignore` omitting the finding. For `hardgate_scan_file`, a read failure is an outer tool error while parse/static findings remain in its per-file report; `hardgate_get_metrics` reports read or missing-symbol errors.

Hardgate complements language linters, formatters, coverage providers, mutation runners, clone tools, and hosted products such as Qlty Cloud. Those tools own their language semantics, execution, or history. Hardgate owns the repository's local policy and the evidence boundary that says what was actually checked.

For the v0.5.0 release contract, the channel contract covers Cargo, the npm
wrapper, and the shell installer. It specifies exactly six Linux/macOS
artifacts (Linux x64/arm64 glibc and musl, macOS x64/arm64), verifies
`SHA256SUMS` and `BUILD-METADATA.json`, and checks the binary's exact version,
target marker, and full source commit; this describes intended release
behavior, not publication already completed. Other Unix targets, Windows, and
Homebrew are not part of this contract.
