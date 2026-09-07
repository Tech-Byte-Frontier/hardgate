# 0.6.4 feedback implementation validation

Validated locally on macOS ARM64 with pinned Rust 1.98.1:

- All 94 tests across 12 focused CLI, configuration, mutation, native execution,
  and parser suites passed, including 10 new feedback regression tests.
- All 23 process/capture/progress/cleanup unit tests passed.
- Clippy with all targets/features and warnings denied passed.
- Repository policy-only check passed with zero findings. This is partial
  acceptance; it does not replace Linux producer evidence or CI.
- Formatting, npm version synchronization, and npm wrapper checks passed.
- The new CLI regression suite also passed on the Rust 1.90 MSRV.

The live-progress regression verifies a heartbeat arrives before the tool exits
and contains its mutation progress excerpt. The TypeScript fixture reproduces
`original<typeof import('react-dom/client')>()`, checks line/column diagnostics,
and verifies the type-alias workaround. Scoped formatting tests cover staged,
unstaged, untracked, deleted, spaced, and shell-metacharacter filenames.

## Existing macOS test limitations to follow up

A full library run had six failures, all reproduced in an untouched export of
0.6.3 (`132633946f931ea45cd639a203e7787849813f69`): four Rust coverage-ownership
fixtures, a Unix-socket fixture denied by the workspace sandbox, and a mutation
lease fixture requiring Linux isolation. The display-contract freshness test
also failed on both revisions because it expects empty stderr despite native
execution diagnostics. These are recorded as existing follow-up work, not
attributed to 0.6.4 or hidden by weakening assertions.

Linux containment, real evidence production, full CI, and publication remain
unverified by this macOS session. The native CI matrix now includes the new
regression suite. Version metadata is prepared for 0.6.4; no release is claimed.
