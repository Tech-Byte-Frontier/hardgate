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
- `hardgate check --diff` scopes ordinary static findings to changed files, compares clones against a full repository index, and scores changed executable LCOV lines; with a legacy ratchet, static and clone comparison uses the full current selected scope (the whole tree when no path filters are supplied);
- `hardgate check --all` adds only formatter/linter/test commands configured by the repository;
- `hardgate verify` runs full static and configured evidence, with optional paths narrowing only static inventory and coverage source matching; mutation reports/freshness/legacy ratchet remain configured/full, without orchestration or native mutation;
- `hardgate mutate` runs a native unmutated baseline and AST mutants when enabled, and prints a disabled-policy note before a successful no-op when disabled.

Enabled evidence is not optional by accident. Empty, missing, unreadable, or malformed reports fail closed. Disabled policies do not read stale report files. A configured legacy reference resolves a Git merge base and can grandfather existing non-worsened static/dead-code debt while keeping new or worsened findings with effective role severity `error` blocking; `warning` findings remain advisories and `ignore` findings are omitted. Coverage, mutation, freshness, and configured orchestration remain current blocking evidence whenever their checks run and are never ratcheted. Stable clone fingerprints and rename lineage preserve safe identities without depending on physical line numbers.

For JavaScript and TypeScript, native mutation validates encountered package
manifests, recognizes only declared workspaces (lockfiles are manager hints),
detects npm, pnpm, Yarn, or Bun, and infers Jest, Vitest, or Playwright only
when selector behavior is unambiguous. A child test script wins; one `test:*`
script is allowed, and a reliable child-local framework package or config
signal wins over a validated enclosing workspace-root script. That root script
is used only with no local script or reliable local signal; malformed manifests
or ambiguous scripts fail closed. It searches for a
matching test and falls back to the full suite. A project-specific
`--test-cmd` remains authoritative. The resolver uses local manager commands
and does not download packages at runtime.

Agents can consume `--format agent` or JSON. MCP is stdio-only and intentionally static-only: `hardgate_check(paths?, diff?)`, `hardgate_scan_file(path)`, and `hardgate_get_metrics(path, symbol)`. `hardgate_check` fails closed on invalid configuration or arguments, missing paths, empty scopes/discovery, unreadable or unparsable files, and Git failures; `hardgate_scan_file` reports read/parse failures in its per-file static report, while `hardgate_get_metrics` reports read or missing-symbol errors.

Hardgate complements language linters, formatters, coverage providers, mutation runners, clone tools, and hosted products such as Qlty Cloud. Those tools own their language semantics, execution, or history. Hardgate owns the repository's local policy and the evidence boundary that says what was actually checked.

For the v0.5.0 release contract, the channel contract covers Cargo, the npm
wrapper, and the shell installer. It specifies six Unix artifacts, verifies
`SHA256SUMS` and `BUILD-METADATA.json`, and checks the binary's exact version,
target marker, and full source commit; this describes intended release
behavior, not publication already completed. The supported installer surface
is Unix; Windows and Homebrew are not part of this contract.
