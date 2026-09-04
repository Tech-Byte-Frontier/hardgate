# Changelog

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
- Prebuilt, npm, and shell-installer distribution is exactly six Linux/macOS
  target artifacts. Remove references to `hardgate-win32-x64`, Homebrew, or
  cargo-dist from existing automation; they are not v0.5.0 release channels.
- The published READMEs now document global npm/pnpm installation and Cargo
  `PATH` troubleshooting without implying that pinned installs auto-update.

The release is intentionally `0.5.0`, rather than `0.4.3`, because Cargo treats
the left-most non-zero component of a `0.y.z` version as its compatibility
boundary.
