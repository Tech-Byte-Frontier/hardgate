# Changelog

## 0.6.1 (2026-09-07)

- Enable local static analysis on macOS, Linux, and Windows. Keep Linux
  containment mandatory for project-tool execution and generated freshness,
  with actionable unsupported-feature errors.
- Add native npm packages and release/consumer validation for Linux ARM64,
  macOS Intel/Apple Silicon, and Windows x64; no Rust is needed for npm installs.
- Test Rust 1.90 as the MSRV independently of the pinned Rust 1.98.1 toolchain.
- Mirror the verified npm wrapper to GitHub Packages after release completion.
- Verify newly published pnpm consumers without the default release-age delay.

- Coordinate CLI and maintenance workloads through one per-user resource slot
  with unique scope identities, cancellable waiting, and inherited lock ownership.

- Add display-only `check --engine`, concise agent triage, deterministic finding
  limits, complete `--report-json` capture, and saved captured-excerpt inspection.
- Keep full verdict/scope/counts and failure context visible through filters;
  explain clone-to-policy execution selection and unsupported source locations.
- Use syntax boundaries to distinguish literal tables and declarative JSX from
  duplicated executable logic, without increasing clone thresholds.
- Accept verified virtualenv interpreter links and disposable verifier caches;
  explain writable temporary paths and separate disposable coverage outputs from
  source-bound producer evidence.

## 0.6.0 (2026-09-06)

- Support is limited to Rust and JavaScript/TypeScript. Python/Go parsers,
  adapters and language rules are removed; unsupported required source remains
  an explicit failure. Related text/config/generated inventory is retained.
- `check` is the combined acceptance command: structural policy, read-only
  formatting/linting, configured tests/type checks, and required evidence.
  `--checks`, `--diff`, path selection and output flags make partial runs explicit. The old
  `verify`, `mutate`, `--all` and generic dead-code interfaces are removed.
- Native mutation is replaced by optional cargo-mutants/Stryker integrations.
  Coverage and mutation receipts bind source, tests, configuration and report
  bytes. Empty, stale, invalid or unfinished evidence blocks acceptance;
  execution and report ingestion remain distinct. Cargo mutation requires a
  successful complete workspace prerequisite and verified restoration.
- Checks execute in protected disposable copies, including when project tools
  enable fixes. Missing tools and ambiguous scripts produce setup failures.
  Explicit formatting remains a separate action. pnpm checks cannot silently
  reinstall dependencies; copied installation metadata may require explicit
  local verifier commands.
- Cargo detection covers workspace members, doctests and declared feature/target
  scope. Clippy diagnostics retain rule, location and individual finding counts.
  Rust cfg(test) ownership separates inline and imported test code from production.
- Remove cognitive complexity, Halstead, ABC and CRAP throughout configuration,
  analysis and reports. Group remaining metrics by function, show code and
  documentation size separately, and count flat else-if chains without artificial
  nesting. Architecture rules remain explicit repository-owned boundaries.
- Preserve diff suppression checks, full-context clone comparison, role policies,
  legacy static ratcheting, real failure propagation and resource containment.
  `init` defaults to balanced adoption; strict-agent/no-config retain required
  coverage and mutation evidence. Existing explicit policy settings remain authoritative.
- Machine output retains schema version 1 and adds acceptance/partial status,
  per-engine execution states, grouped review targets and specialist diagnostics.
  Exit 1 identifies violations; exit 2 identifies incomplete evaluation/setup.
  Invalid obsolete configuration is rejected instead of silently ignored.
- Future releases target Linux x64 GNU only through Cargo, direct archives,
  npm and pnpm. Remove the shell installer and unused platform packages/matrices.
  CI reuses one supported binary, validates actual installed checks, and keeps
  signed tags, checksums, provenance, immutable artifacts and same-tag recovery.
- Real Rust and JS/workspace trials document useful observations and remaining
  noise. The retained ripgrep ByteSet integration reproduces a surviving mutant,
  a passing new assertion and that same mutant being caught, with restoration.
  See [trial results](docs/TRIALS_0_6.md).

This is a breaking pre-1.0 compatibility release. Migrate CLI/config/report and
public Rust integrations using the configuration and command guides. Existing
0.5.0 artifacts remain immutable. Publication uses the signed, CI-validated release workflow.

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
