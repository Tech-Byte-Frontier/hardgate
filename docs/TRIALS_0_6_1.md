# 0.6.1 implementation trials

Local validation on 2026-09-06 against installed 0.6.0 at
`d0393f7061cb8d1f143ca8525f53505e7cd4b855`. The implementation trials used patched
local builds labeled 0.6.0; source manifests are now 0.6.1 (unreleased).
This is proportional implementation evidence;
full release coverage, native mutation and packaging remain a separate step.

## Clone triage

Both versions used the same custom policy with unchanged detection defaults
(5 lines, 50 tokens). These were partial `--checks policy` runs, with exit 1
and `accepted: false` in both versions. The TypeScript input was a disposable
snapshot of finance-cli's `web/src` and `web/package.json` at
`60e31cd38abdf526f51602ccca641c15c295033d`; its live checkout was not modified.

Before: `hardgate check --checks policy --json | jq ...` extracted both clone
ranges and line/token counts. After:

```sh
hardgate check --checks policy --engine clones --compact --report-json full.json
hardgate report full.json --engine clones --max-diagnostics 1 --json
```

The first command includes both ranges, line/token counts, the original verdict,
execution scope and totals. The second inspects saved evidence without execution.
The old pipeline requires `set -o pipefail` to preserve Hardgate's failing exit.

| Input | Triage commands, old/new | Clone pairs, old/new | Triage stdout bytes, old/new | Same saved findings rendered as agent, old/new bytes |
| --- | --- | --- | --- | --- |
| Rust fixture: two duplicated functions | 2 / 1 | 1 / 1 | 61 / 787 | 599 / 932 |
| TypeScript snapshot: 247 inventory files, 2,128 functions | 2 / 1 | 158 / 52 | 20,337 / 9,230 | 54,650 / 39,766 |

The TypeScript compact reduction includes changed clone precision. Rendering the
same old report isolates the presentation reduction (27.2%). The small Rust
report grows because it now retains acceptance context. These are byte counts,
not a developer-time study or an exhaustive audit of the removed findings.
Source/config hashes matched before and after both trials; bounded inspection
preserved original totals and exit codes.

## Reproductions and regressions

- A 20-row multiline literal object table self-matched on 0.6.0 (one pair,
  exit 1); the implementation returned zero pairs. Regression cases cover
  15/35/600 rows, tuple/object tables, overlapping candidates, and Rust tuples.
- Repeated declarative chart JSX reproduced false positives. Syntax boundaries
  remove literal data and declarative markup from clone candidates; handlers,
  expressions, callbacks and following executable code retain positive controls.
  Global thresholds were unchanged; no wrapper-specific threshold was added.
- Real ignored uv and `python -m venv` environments, plus a Python virtualenv in
  a `.tox/py` layout, changed from external-interpreter setup failure (exit 2) to
  successful configured interpreter execution (exit 0). This does not test the
  tox runner itself. Every trial retained source/config hashes and symlink targets.
- Public CLI tests cover unsupported `.py` file locations alongside YAML/CSS
  inventory, clone-selector guidance, mixed findings, advisories, empty filters,
  zero limits, partial success, tool failures and missing evidence. Captured
  snippets survive saved inspection; changed live source is never substituted.
- Lifecycle regressions allow recognized Ruff/import-linter/pytest/ESLint/pyc
  cache records, reject edits to ignored required inputs and arbitrary external
  links, and verify writable runtime `$TMPDIR` guidance with containment intact.
- Declared LCOV output can change only in the disposable check copy. Producer
  tests independently verify valid/stale/tampered original receipts and reports;
  test-produced bytes cannot refresh evidence or modify original source/config.

## Validation

The serialized all-target test run completed 62 targets and exposed seven
presentation assertions across five targets. Those contracts were corrected
and all affected targets passed on rerun (322 tests in that regression batch).
Subsequent focused runs passed CLI triage (8), execution lifecycle (5), clone
precision (4), producer/evidence (16), diagnostic rendering (2), and report
command/review/all-engine tests (24). No failing test remains known.

Final stable-toolchain checks passed: `cargo fmt --check`,
`cargo clippy --locked --all-targets -- -D warnings`, and
`hardgate check --checks policy --diff --json` with zero blocking findings.
The static check is explicitly partial and retains advisories. Final acceptance
trials also passed. Commands, raw outputs, binary SHA-256 identities and hash
manifests are retained locally under `/tmp/hg061-acceptance` and `/tmp/hg061-*.log`.

## Concurrency and installed-consumer follow-up

The reproduced `hardgate-workload.scope` contention is now fixed. CLI and
maintenance supervisors use unique scope identities and the same per-user flock
slot. The scope leader inherits the lock; another workload cannot start while
that leader survives a terminated outer supervisor. Resource limits and the
separate mutation lease are unchanged.

Real systemd trials cover overlapping repositories and maintenance runners,
queued/active cancellation, SIGKILL of either supervisor, immediate restart,
literal command arguments and original source restoration. CI runs these tests
directly so an inherited test-runner scope cannot hide supervisor defects.
Resource library tests (131), readiness tests (8), and public containment tests
(2) passed. Environment lifecycle tests now total 6, including an inherited
external `UV_CACHE_DIR` redirected into disposable storage. Producer tests (16)
still pass. Node resource-boundary, ownership/locking and repository-rule checks
passed; full-source static policy and all-target Clippy passed after refactoring.

The optimized local Cargo installation was tested against finance-cli at
`3a424ee` with its real workspace `.venv`. FastAPI/uvicorn imports and
`uv run finance up --dry-run` passed without the API extra. Its configured
formatting, linting and test commands completed through Hardgate in 127.51 seconds
after removing external-venv and no-cache workarounds; tracked source/config
hashes matched. That check explicitly selected `format,lint,tests`, so it remains
partial (`accepted: false`); this is not a complete finance-cli policy/evidence gate.
Detailed consumer, concurrency and installation evidence is retained in
`/tmp/hg061-followup` and `/tmp/hg061-resource-*.log`.
The tested binary was installed atomically at `~/.cargo/bin/hardgate`; its bytes
match the staged Cargo installation. The previous binary is retained for rollback.
