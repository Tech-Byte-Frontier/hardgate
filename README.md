# Hardgate

**Deterministic quality gates, structural budgets, and anti-gaming checks for agent-assisted software.**

[![Crates.io](https://img.shields.io/crates/v/hardgate.svg)](https://crates.io/crates/hardgate)
[![Documentation](https://docs.rs/hardgate/badge.svg)](https://docs.rs/hardgate)
[![License](https://img.shields.io/crates/l/hardgate.svg)](https://github.com/Tech-Byte-Frontier/hardgate/blob/v0.5.0/LICENSE-MIT)

Hardgate is a local Rust CLI. It turns repository policy into a deterministic report that a maintainer, CI job, or coding agent can inspect before accepting a change. A passing report means that the enabled engines found no blocking findings; it is not a claim that every quality property has been proven.

## First run

### Current source checkout (unreleased)

The initialization and `config` inspection flow below is in this checkout and
has not shipped in a release. From the exact checkout that contains this
implementation, install the binary, then change to the project you want to
initialize. Replace the placeholder path before running the commands:

```sh
cargo install --path . --locked
# Replace this placeholder with the repository you are initializing.
cd /path/to/your/project
hardgate init --preset balanced
hardgate config
hardgate check
```

`balanced` is an explicitly structural starting point. Initialization reads
project-root manifest and configuration contents to choose relevant commands;
it does not install tools or execute project commands. It creates
`hardgate.toml` with create-new semantics, so an existing file (including a
broken symlink) is left untouched. Review the completion summary for enabled
engines, missing setup, and the next command. An arbitrary existing repository
may still fail its first check because its source roles, budgets, commands, or
evidence need project-specific decisions.

The same checkout also provides inspection-friendly initialization:

```sh
# Run from the same source checkout containing the current init implementation.
cargo run --release -- init --preset balanced --preview > /tmp/hardgate.toml
cargo run --release -- init --preset balanced --preview --full > /tmp/hardgate-effective.toml
cargo run --release -- config --format toml
```

`--preview` writes only valid generated TOML to stdout; its status and completion
summary go to stderr, so the output can be redirected safely. `--full` renders
the full effective policy. See the current [`init`](src/commands/init.rs) and
[`config`](src/commands/inspect.rs) implementations.

Without a policy file, the CLI uses the `strict-agent` defaults. That preset
requires configured LCOV and mutation reports. `legacy-migration` adds a
static reference ratchet for adoption, while `custom` starts from ordinary
deserialized defaults and expects the policy file to state the project choices.

### See a pass, a diagnostic, and a real refactor

This small Rust fixture demonstrates the structural loop. It is intentionally
bounded and says nothing about whether an arbitrary existing repository will
pass:

```sh
smoke_dir="$(mktemp -d)"
mkdir -p "$smoke_dir/src"
cat > "$smoke_dir/Cargo.toml" <<'EOF'
[package]
name = "hardgate-smoke"
version = "0.1.0"
edition = "2021"
EOF
cat > "$smoke_dir/src/lib.rs" <<'EOF'
pub fn add(left: u32, right: u32) -> u32 {
    left + right
}
EOF
cd "$smoke_dir"
hardgate init --preset balanced
hardgate config
hardgate check --format compact
```

To see a parameter-budget diagnostic, temporarily replace `src/lib.rs` with a
seven-parameter function. The balanced preset's default parameter ceiling is
six, so this check should report a parameter-budget violation:

```rust
pub fn sum(a: u32, b: u32, c: u32, d: u32, e: u32, f: u32, g: u32) -> u32 {
    a + b + c + d + e + f + g
}
```

A real refactor can make the data explicit and return to one parameter:

```rust
pub fn sum(values: [u32; 7]) -> u32 {
    values.into_iter().sum()
}
```

Run `hardgate check --format compact` again. The fixture should pass its
structural gate after the refactor. `check` is structural and does not execute
the detected formatter, linter, or test commands; use `check --all` only after
reviewing those configured commands.

### Strict evidence and command boundaries

`strict-agent` enables strict structural thresholds plus LCOV coverage and
mutation-report evidence. Both `hardgate check` and `hardgate verify` require
real, non-empty reports when those engines are enabled. The policy remains
incomplete until a project generates the reports; initialization only describes
missing producers and paths. `hardgate config` prints the validated effective
policy and is the inspection step before adding those producers.

`hardgate mutate` is a separate native mutation workflow: it runs an
unmutated baseline and then bounded mutants against a test command. That
baseline-plus-sample result is separate from configured mutation-report
ingestion. `hardgate check --all` runs only the configured formatter, linter,
and test commands. `hardgate verify` does not run those project commands or
native mutation; the only external command it may run is an enabled generated-
freshness check.

### Install a released CLI

Published channels are listed here for released installations: the Rust CLI is
available as Cargo `0.5.0`, and the [GitHub release](https://github.com/Tech-Byte-Frontier/hardgate/releases/tag/v0.5.0)
is `v0.5.0`. The npm wrapper remains at `0.4.2`.

```sh
cargo install hardgate --version 0.5.0 --locked
hardgate --version
```

The [npm wrapper](https://www.npmjs.com/package/@tech-byte-frontier/hardgate) is
published separately and currently remains at `0.4.2`. Pin that version when
using a JavaScript package manager; the unpublished `0.5.0` wrapper manifests
in this source tree are not an npm install target:

```sh
npm install --save-dev --save-exact @tech-byte-frontier/hardgate@0.4.2
npx hardgate --version

pnpm add --save-dev --save-exact @tech-byte-frontier/hardgate@0.4.2
pnpm exec hardgate --version

yarn add --dev --exact @tech-byte-frontier/hardgate@0.4.2
yarn exec hardgate --version

bun add --dev --exact @tech-byte-frontier/hardgate@0.4.2
bunx --no-install hardgate --version
```

For a global npm or pnpm command, use the same published wrapper version:

```sh
npm install --global @tech-byte-frontier/hardgate@0.4.2
# or
pnpm add --global @tech-byte-frontier/hardgate@0.4.2

hardgate --version
```

The npm wrapper requires Node.js 18 or newer.

For npm, `npm prefix --global` prints the global prefix; its `bin` directory
must be on `PATH`. For pnpm, run `pnpm setup` if it reports that no global bin
directory is configured, then open a new shell so `PNPM_HOME` is on `PATH`.
The wrapper's supported platform packages and `HARDGATE_BINARY` resolution are
documented in the [wrapper README](npm/hardgate/README.md).

For a source build of the released tag, use the tag explicitly:

```sh
git clone --branch v0.5.0 https://github.com/Tech-Byte-Frontier/hardgate.git
cd hardgate
cargo install --path . --locked
```

Cargo installs the executable under the selected install root's `bin`
directory. `--root` takes precedence, followed by `CARGO_INSTALL_ROOT`, Cargo's
`install.root` setting, and `$CARGO_HOME` (normally `$HOME/.cargo`). With
rustup, `. "$HOME/.cargo/env"` loads the standard `$HOME/.cargo/bin` path;
verify the resolved binary with `command -v hardgate` and `hardgate --version`.

### Uninstall

Use the command matching the installation channel:

```sh
cargo uninstall hardgate
npm uninstall --save-dev @tech-byte-frontier/hardgate
pnpm remove --save-dev @tech-byte-frontier/hardgate
yarn remove @tech-byte-frontier/hardgate
bun remove @tech-byte-frontier/hardgate
npm uninstall --global @tech-byte-frontier/hardgate
pnpm remove --global @tech-byte-frontier/hardgate
```

## What is enforced

- **Role-aware discovery.** Inventory files are classified as source, test, generated, fixture, migration, configuration, documentation, vendor, or unknown before engines choose their inputs. Dependency and build-output directories are pruned by default. File-budget and clone exclusions belong only to their owning engines and remain visible as advisories; dead-code exclusions are engine-local and silent.
- **Structural budgets.** Tree-sitter metrics cover Rust, JavaScript, TypeScript, TSX, Python, and Go. File bytes/lines and per-function cyclomatic, cognitive, Halstead, ABC, parameter, statement, body-line, and nesting ceilings are configurable.
- **Anti-gaming policy.** Known suppression directives and project-forbidden tokens can be blocking findings on safety-checked roles. The current configuration has no inline approval or suppression exception channel.
- **Architectural boundaries.** Declarative path-scoped rules inspect import strings, call names, and tokens. This is a local rule scanner, not module resolution or type checking.
- **Clone debt.** Bounded token windows compare eligible files using verified normalized token sequences. Current clone findings carry a stable content fingerprint that does not include path or line numbers, so rename lineage can be matched during legacy adoption.
- **Evidence.** Enabled LCOV coverage and JSON mutation reports are required inputs. Empty, missing, unreadable, or malformed required evidence is a blocking finding. Disabled engines do not consume stale report files. Generated-artifact freshness is a separate required check when enabled. This repository generates its branch LCOV with the pinned `RUST_COVERAGE_TOOLCHAIN` (`nightly-2026-09-04`) because Rust branch instrumentation is unstable; the report includes executable `build.rs` coverage, while Rust 1.98.1 remains the crate MSRV and normal build/test toolchain.
- **Native mutation.** When `[mutation].enabled = true`, `hardgate mutate` copies current inputs into a private workspace, runs a real unmutated baseline before bounded AST mutants, classifies outcomes, and rejects a selection with no viable mutants. SIGINT/SIGTERM stop owned tests and clean up the copy; original source stays intact even when SIGKILL prevents cleanup. See [workspace and dependency behavior](docs/CLI_AND_INTEGRATION.md#hardgate-mutate). With mutation disabled it succeeds without discovery or execution. Native mutation supports Linux/macOS; other source-build platforms fail closed before tests or mutation. Static `check` and `scan` remain independent.
- **Orchestration.** `check --all` runs only formatter, linter, and test commands configured by the repository. Hardgate never invents a command or treats an unconfigured test suite as evidence.

Invariant checking is enabled by default; with no configured rules it has nothing to report. Set `[invariants].enforce = false` to disable it explicitly.

### Inventory and parser support

Tree-sitter parsing covers:

| Family | Extensions |
| --- | --- |
| Rust | `.rs` |
| JavaScript | `.js`, `.jsx`, `.mjs`, `.cjs` |
| TypeScript | `.ts`, `.mts`, `.cts` |
| TSX | `.tsx` |
| Python | `.py` |
| Go | `.go` |

The inventory also records `.css`, `.mdx`, `.sql`, `.json`, `.jsonc`,
`.graphql`, `.gql`, `.snap`, `.toml`, `.yaml`, and `.yml`; these formats
remain visible to classification and applicable safety rules but do not
receive function metrics. Inventory is not a claim of parser support: with
the preset role severities, a parser-unsupported file that remains classified
as source or migration blocks with `unsupported-source`. This includes
handwritten CSS, GraphQL, and non-migration SQL unless the project makes an
explicit, truthful classification or role-policy decision. Markdown (`.md`)
is not a built-in inventory extension.

### Node and Supabase conventions

Source and test files with the JavaScript-family extensions above receive
Tree-sitter metrics; `.mjs`, `.cjs`, `.mts`, and `.cts` are included. Built-in
classification marks `supabase/database.types.ts` and
`supabase/schema.gen.ts` as generated, while `supabase/functions/**/*.ts` is
source. `supabase/migrations/**/*.sql`, `supabase/seed.sql`, and
`*.migration.sql`/`*.seed.sql` are migration files without an AST parser;
`supabase/seed.ts` is also migration-role but has TypeScript parser support.
Migrations receive safety policy rather than ordinary source/test complexity
or native mutation. Under the default strict migration policy, only the
parser-unsupported migration files produce a blocking `unsupported-source`
finding. A custom classification rule may assign another role, but it does not
add a SQL parser. Other
Supabase configuration/data files (for example `supabase/config.toml`) are
inventoried as configuration and likewise have no function metrics.

## Commands and evidence boundaries

```sh
# Static engines plus enabled report and generated-freshness evidence.
hardgate check

# Git-changed/staged static scope by default; explicit existing paths add to
# static/clone selection. With a legacy ratchet, static/clone comparison uses
# the full current selected scope (whole tree when no paths are supplied).
# LCOV always intersects actual changed executable source lines.
hardgate check --diff

# Add configured formatter, linter, and test commands.
hardgate check --all

# Opt in to configured dead-code analysis.
hardgate check --dead-code

# Full static/dead-code verification plus enabled reports, freshness, and legacy
# ratchet; path filters only narrow current static/dead-code inventory and
# coverage source matching.
hardgate verify

# Native baseline + AST-mutant execution (not report ingestion).
hardgate mutate --scoped src/lib.rs --test-cmd 'cargo test'

hardgate scan src/lib.rs
hardgate fmt --check
hardgate check --format agent
hardgate check --format json
```

Use `hardgate check --all` as the normal CI entry point after configuring the
repository's formatter, linter, and tests. Plain `check` remains the fast,
non-orchestrating command for local and agent feedback loops. Under
`strict-agent`, both `check` and `verify` require real LCOV and mutation reports
when those evidence engines are enabled; a generated policy is incomplete until
those reports exist. Native `hardgate mutate` is a separate baseline-plus-sample
run and does not replace configured mutation-report ingestion.

The command contract is:

| Command | Runs | Does not run |
| --- | --- | --- |
| `check` | Static engines, enabled coverage/mutation reports, enabled generated freshness; optional configured dead code | Formatter/linter/test orchestration unless `--all`; native mutation |
| `check --diff` | Git-changed/staged static files by default; explicit existing paths add to static/clone selection, with full-index clone matching; with a ratchet, static/clone analysis uses the full current selected scope (whole tree when no paths are supplied). LCOV always intersects actual changed executable lines | Native mutation; orchestration unless `--all` |
| `check --all` | Everything in `check` plus configured orchestration steps | Native mutation |
| `verify` | Full-tree static/dead-code and configured evidence by default; path filters scope only current static/dead-code inventory and coverage source matching, while mutation reports and freshness remain configured/full. The ratchet loads the full configured reference snapshot but compares only selected current static/dead-code findings. Only enabled generated-freshness checks may run an external command. | Formatter/linter/test orchestration and native mutation |
| `mutate` | When enabled, native unmutated baseline and bounded mutants; when disabled, a note and successful no-op | Coverage/mutation report ingestion |

`verify` path arguments do not narrow mutation-report ingestion or generated
freshness; those remain configured/full checks. The ratchet still loads and
validates the full configured reference snapshot, then compares it only to the
selected current static/dead-code findings; explicit paths do not widen that
current selection.

Enabled required evidence fails closed when it is missing or empty. CLI `check` and `verify` retain an empty-discovery advisory and still run every enabled report, freshness, and legacy gate; the MCP `hardgate_check` surface rejects empty scopes/discovery instead of returning a successful empty report. Missing or malformed Git evidence, coverage/mutation reports, generated freshness commands, and mutation outcomes are failures in the corresponding path; a valid Git worktree with no changed files is an advisory/no-op for diff selection. Disabled evidence engines do not inspect old report files. See the [CLI reference and agent integration](docs/CLI_AND_INTEGRATION.md) for details.

When native mutation is enabled, it requires a source-role target and at least
one viable mutant. After an explicit scope is validated, a `mutate --diff`
invocation with no changed production source is an explicitly reported no-op,
including when `--scoped` is supplied. Missing, invalid, unsupported, or
non-source explicit scopes fail closed. Only a non-diff unrestricted or scoped
run with no eligible target fails.

## Roles, legacy adoption, and clones

The first-class role policies (`roles.source`, `roles.test`, `roles.generated`, `roles.fixture`, and `roles.migration`) are independent. Each can set severity (`error`, `warning`, or `ignore`), file/function ceilings, and clone thresholds. Native mutation is source-role-only: source mutation eligibility is configurable, while non-source roles must remain ineligible. Built-ins classify generated files and fixtures before ordinary source conventions; ordered `[classification.rules]` may override built-ins except for vendor/build pruning.

Generated freshness is intentionally separate from file-budget exclusions. Excluding a generated path from byte/line checks does not disable its configured freshness command. Freshness failures remain current blocking evidence and are not grandfathered by legacy adoption.

When `[legacy].ratchet = true`, Hardgate resolves the configured reference and merge base, analyzes the baseline static snapshot (plus configured dead code), and compares it with the current static report. Existing non-worsened static debt can be grandfathered as advisories; new or worsened findings with effective role severity `error` remain blocking, `warning` findings remain advisories, and `ignore` findings are omitted. Git rename lineage and line-independent clone fingerprints preserve identity across safe renames. Retained findings include changed-file or changed-hunk context. Coverage, mutation, generated freshness, and configured orchestration findings remain current blocking evidence whenever their checks run and are never ratcheted.

## MCP and agent integration

`hardgate mcp` serves MCP over standard input/output. The static-only `hardgate_check(paths?, diff?)` tool routes through the same static gate as the CLI. It accepts optional path strings and a boolean `diff`; Git-changed/staged inventory is the default diff selection, while explicit existing paths add to static/clone selection and clone matching uses the full repository index. MCP never runs coverage or other report/freshness/orchestration/dead-code/native-mutation engines. Invalid arguments/configuration, missing paths, empty scopes/discovery, and Git failures are outer tool errors. Read/parse failures remain report-level Hardgate `Failed` findings: effective role severity `error` fails the report, `warning` is an advisory, and `ignore` omits the finding. For `hardgate_scan_file`, a read failure is an outer tool error while parse/static findings remain in its per-file report; `hardgate_get_metrics` reports read or missing-symbol errors explicitly.

The other tools are `hardgate_scan_file(path)` and `hardgate_get_metrics(path, symbol)`. Register the server with an MCP client:

```json
{
  "mcpServers": {
    "hardgate": {
      "command": "hardgate",
      "args": ["mcp"]
    }
  }
}
```

## Build identity

Release archives carry `BUILD-METADATA.json` with the binary name, numeric
version, Cargo target triple, npm package name, and full source commit. Each
binary embeds `hardgate-target:<target>` and reports exactly
`hardgate VERSION (COMMIT)` for `--version`; release verification checks the
checksum, metadata, target marker, and identity, while the installer checks
the archive metadata and binary version/commit before installation.

## Documentation

- [Vision and paradigm](docs/VISION_AND_PARADIGM.md)
- [Configuration specification](docs/CONFIGURATION_SPEC.md)
- [CLI reference and agent integration](docs/CLI_AND_INTEGRATION.md)
- [System architecture](docs/ARCHITECTURE.md)
- [Existing landscape](docs/EXISTING_LANDSCAPE.md)
- [X article](docs/X_ARTICLE.md)
- [API reference](https://docs.rs/hardgate)

## License

Dual-licensed under either [Apache License 2.0](https://github.com/Tech-Byte-Frontier/hardgate/blob/v0.5.0/LICENSE-APACHE) or [MIT](https://github.com/Tech-Byte-Frontier/hardgate/blob/v0.5.0/LICENSE-MIT), at your option.
