# Changelog

## 0.6.0 (unreleased)

- `init` defaults to balanced structural adoption; explicit strict-agent and
  no-config execution retain required 95/95/90 coverage and 85% mutation evidence.
  Strict-agent allows five parameters; balanced/legacy allow 50 statements.
  Test size and duplication are visible advisories while complexity and safety
  still block. Source clone detection remains sensitive, with separate blocking
  minimums (strict 10 lines/100 tokens; balanced/legacy 15/150). Category severity
  overrides preserve deliberate stricter policies. Existing explicit settings
  win; omitted preset fields inherit these updated defaults. Legacy verdicts
  identify adoption scope and do not certify removal of historical debt.

- Machine-readable gate, mutation, no-op, config, and error outputs use
  `schema_version: 1`; execution records distinguish `disabled`, `skipped`,
  `incomplete`, `failed`, and `completed` evidence.
- Exit status is 0 for pass/no-op, 1 for policy violations, and 2 when required
  evidence or configuration cannot be evaluated. Complete verdict/summary counts survive bounded
  `--max-diagnostics`/`--snippets` output; stable rule IDs replace message parsing and snippets use
  only captured source bytes.
- `check --diff` indexes the full eligible repository for clones and reports
  changed/reference context, catching copies against unchanged files; dead-code analysis retains
  required repository reference context.
- Native mutation serializes workloads across projects for the same user, caps
  common build-worker defaults, checks Linux memory pressure and applies aggregate
  Linux memory limits when an eligible systemd user manager is available. Resource
  failures remain incomplete evidence. Snapshot copying uses bounded buffers.
- The README is a concise entry point with linked installation and getting-started guides.
- Native `mutate` runs in a private workspace, restores and verifies source bytes
  after each mutant, and cleans up owned processes and temporary files. Mutation report ingestion
  remains a separate engine.
- Policy discovery uses the nearest `hardgate.toml` up to the Git boundary;
  absent policy defaults to `strict-agent` at the Git root. Fixed configuration tables reject
  unknown keys, while `hardgate config` exposes effective policy, root, invocation directory, and identity.
- Public command APIs return `CommandOutcome`/`CommandResult` and structured
  reports/errors. `init` is project-aware and non-destructive; use `--preview` or `--full`, and add explicit
  commands for ambiguous or nested projects. Shell completions, `--threads`, `--timing`, effective-policy
  inspection, and bounded diagnostics are available; migrate consumers to documented roots, schema/status
  checks, exit 1 versus 2, additive fields, and stable rule IDs.
- Local unreleased release tooling drafts staged identity-bound receipts with
  exact-version-before-default checks, explicit npm auth modes, and independent native/registry/consumer
  verification. This is review-only local capability; no publication or external settings change is claimed.

This is a new `0.6.0` compatibility release: public Rust command result types,
JSON schema/status contracts, and CLI exit meanings changed since `0.5.0`.
Update integrations to distinguish policy failure (exit 1) from incomplete
evaluation (exit 2), check `schema_version`, and read engine execution states.
Existing `0.5.0` artifacts remain immutable; these changes must not be republished
under that version.

## 0.5.0

Hardgate 0.5.0 is the pre-1.0 compatibility boundary for the stabilization
work. The minimum supported Rust version is now 1.98.1 (up from 1.85).

Migration notes:

- Existing policies should start from a freshly generated preset and merge
  intentional overrides. Roles, classification, generated freshness, legacy
  ratcheting, and orchestration timeouts are now first-class configuration.
  The removed `mutation.reject_timeouts` key is rejected; timeouts always fail.
- Enabled coverage and mutation evidence now fail closed when required input is
  absent, empty, unreadable, malformed, or unviable. Zero viable mutants score
  zero. File-budget exclusions no longer remove files from other engines.
- The no-config and `strict-agent` policies enable coverage and mutation-report
  evidence. Projects that intentionally need structural-only checks should use
  `balanced` or explicitly set both `[coverage].enabled = false` and
  `[mutation].enabled = false`; do not carry `mutation.reject_timeouts` forward.
- Public Rust consumers must update for the expanded configuration and result
  types, fallible JSON/clone APIs, detailed mutation outcomes, clone
  fingerprints, and command-module re-exports.
- `hardgate --version` now includes the full source commit. JSON output takes
  precedence when requested and mutation resolution errors are typed failures.
- Built-in AST metrics cover Rust, JavaScript/TypeScript/TSX, Python, and Go.
  C/C++ is not advertised as supported. Native mutation is compiled into the
  six Linux/macOS prebuilt/npm binaries; other target operating systems fail
  closed before mutation execution or source writes.
- Inventory is broader than AST support. Parser-unsupported files that remain
  source or migration (including CSS, GraphQL, and SQL) block as
  `unsupported-source` under preset role severities; accepting them requires
  an explicit classification or role-policy decision.
- Prebuilt, npm, and shell-installer distribution is exactly six Linux/macOS
  target artifacts. Remove references to `hardgate-win32-x64`, Homebrew, or
  cargo-dist from existing automation; they are not v0.5.0 release channels.
- The published READMEs now document global npm/pnpm installation and Cargo
  `PATH` troubleshooting without implying that pinned installs auto-update.

The release is intentionally `0.5.0`, rather than `0.4.3`, because Cargo treats
the left-most non-zero component of a `0.y.z` version as its compatibility
boundary.
