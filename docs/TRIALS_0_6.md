# 0.6 implementation trials

Local working-tree validation on 2026-09-05; this is not a published release or
complete release gate. Public project revisions are fixed below. Balanced policy
was unchanged: no threshold reductions or source exclusions. External checkout
inputs and temporary policy edits were restored. Detailed commands, reports,
receipts and timings are retained in `/tmp/hardgate-value-20260905`.

| Project/revision | Existing tools | Extra Hardgate observations |
| --- | --- | --- |
| shell-words `8d2868b0` | 13 unit tests + 2 doctests and rustfmt passed; Clippy found 7 diagnostics | One structural review target (`split`), alongside the same 7 individually located Clippy findings; combined check 0.464s |
| ripgrep `3fce3b5b` | Default workspace suite: 1,229 tests/doctests passed, 3 ignored; rustfmt passed; all-feature Clippy found 9 diagnostics | 74 structural findings: 28 function targets, 20 file budgets, 18 suppressions, 4 clones. Static scan 0.464s; combined check 24.844s. Its all-target test command additionally exposes a nightly-only benchmark, so combined acceptance fails |
| clsx `925494cf` | Existing uvu/esm tests fail with an import SyntaxError on tested Node 22/26 environments | No extra structural findings; missing formatter/linter and the existing test failure block acceptance; static 0.064s, combined 0.364s |
| tinybench workspace `b3be2974` | ESLint passed in 4.622s; TypeScript in 0.966s; Vitest 243 tests/64 files in 80.542s | 86 structural findings, including two handwritten function targets; generated documentation bundle produces noisy findings and exhausts clone capacity. Static 0.064s; acceptance remains blocked |

Structural observations are not demonstrated bugs. A bounded agent review of five
function targets plus documentation/generated-file findings took 27.89 seconds;
this is not a human usability study or an exhaustive false-positive audit. The
state machine in shell-words and runtime dispatch in tinybench are cohesive despite
high branch counts. Ripgrep argument conversion and tinybench result processing
are plausible review targets. Tinybench's 808-line types file contains 481
documentation lines and 234 code lines; the breakdown prevents interpreting it as
808 executable lines. Generated bundle findings and deliberate suppressions add
review cost; they were retained in the failing report.

Setup: generated `init` took 3ms/project. Dependency installation reported 1s for
clsx and 7s for tinybench; Rust used a verified offline cache. Tinybench needs
explicit lint/test/type-check commands and still lacks a formatter. pnpm 11 sees
copied workspace installation metadata as stale: checks now return its dependency
error instead of attempting installation. Direct local verifiers completed ESLint, Vitest and TypeScript through Hardgate
in 87.428s, with the same tests and source-write protection; the two remaining
blockers are formatter setup and generated-bundle clone capacity. Rust
all-target checks can require a project-specific nightly benchmark command.
These timings exclude downloads, diagnosis and implementation work; no overall
setup-time or developer-time savings claim is established.

Trials exposed and fixed Hardgate defects: Rust cfg(test) projection could create
invalid syntax; flat else-if chains inflated nesting; cargo-mutants package-only
baselines missed failing workspace members; pnpm cache writes were classified as
source edits. Regression tests cover these boundaries. No new analyzer was added.

The reproducible ByteSet integration uses cargo-mutants 27.1.0. One selected
`ByteSet::add` no-op survived all workspace tests (47.54s). A byte-insertion
assertion passed on original code (3.71s) and killed the identical mutant (45.97s).
Both receipts verified successful workspace prerequisites, structured outcomes,
report identity and restoration. The harness retains the complete runner listing
and selects this sample by shard because 27.1.0 leaks struct-field deletions through
its regex filter. See [the producer contract](CLI_AND_INTEGRATION.md#source-bound-evidence)
and `scripts/check-ripgrep-mutation.mjs`. This proves one missing assertion;
it does not establish a repository-wide mutation score.
