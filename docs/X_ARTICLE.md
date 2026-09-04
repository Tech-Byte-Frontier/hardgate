# Hardgate: an evidence contract for agent-assisted code

Coding agents are good at producing a plausible patch. They are also good at finding the shortest route to a green command: a suppression pragma, a copied block, an unscanned generated file, or a report that no longer describes the current tree.

Hardgate makes the acceptance rule explicit and local. It inventories files, classifies repository roles, applies structural budgets and anti-gaming checks, evaluates configured evidence, and emits one report for a maintainer or an agent.

The important distinction is not “AI versus humans.” It is probabilistic generation versus deterministic policy:

```text
agent proposal -> repository policy -> evidence-backed report
```

The policy is role-aware. Source, test, generated, fixture, and migration roles have independent severity, budget, clone, and mutation-target settings. Generated files can be excluded from handwritten debt checks without disabling a separate freshness command. A file-budget exclusion therefore cannot quietly turn off generated-artifact verification.

The command boundaries are deliberate:

- `hardgate check` runs static engines plus enabled coverage/mutation reports and freshness;
- `hardgate check --diff` scopes static findings to changed files, compares clones against a full repository index, and scores changed executable LCOV lines;
- `hardgate check --all` adds only formatter/linter/test commands configured by the repository;
- `hardgate verify` runs full static evidence, enabled reports/freshness, and the legacy static ratchet, without orchestration or native mutation;
- `hardgate mutate` runs a native unmutated baseline and AST mutants.

Enabled evidence is not optional by accident. Empty, missing, unreadable, or malformed reports fail closed. Disabled policies do not read stale report files. A configured legacy reference resolves a Git merge base and can grandfather existing non-worsened static/dead-code debt while keeping new or worsened debt blocking. Coverage, mutation, freshness, and configured orchestration remain current blocking evidence whenever their checks run and are never ratcheted. Stable clone fingerprints and rename lineage preserve safe identities without depending on physical line numbers.

For JavaScript and TypeScript, native mutation resolves the nearest package and workspace markers, detects npm, pnpm, Yarn, or Bun, infers Jest, Vitest, or Playwright, searches for a matching test, and falls back to the full suite. A project-specific `--test-cmd` remains authoritative. The resolver uses local manager commands and does not download packages at runtime.

Agents can consume `--format agent` or JSON. MCP is stdio-only and intentionally static-only: `hardgate_check(paths?, diff?)`, `hardgate_scan_file(path)`, and `hardgate_get_metrics(path, symbol)`. Invalid configuration, missing paths, empty scopes, parser/read/Git failures, and empty discovery return an explicit failure rather than a green empty report.

Hardgate complements language linters, formatters, coverage providers, mutation runners, clone tools, and hosted products such as Qlty Cloud. Those tools own their language semantics, execution, or history. Hardgate owns the repository's local policy and the evidence boundary that says what was actually checked.

Install from Cargo, the npm wrapper, or the shell installer. The release contract contains six Unix artifacts, verifies `SHA256SUMS` and `BUILD-METADATA.json`, and checks the binary's exact version and full source commit. The supported installer surface is Unix; Windows and Homebrew are not part of this contract.
