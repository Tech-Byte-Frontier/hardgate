# Hardgate

**Deterministic quality gates, structural budgets, and anti-gaming checks for agent-assisted software.**

[![Crates.io](https://img.shields.io/crates/v/hardgate.svg)](https://crates.io/crates/hardgate)
[![Documentation](https://docs.rs/hardgate/badge.svg)](https://docs.rs/hardgate)
[![License](https://img.shields.io/crates/l/hardgate.svg)](https://github.com/Tech-Byte-Frontier/hardgate#license)

Hardgate is a local Rust CLI. It turns repository policy into a deterministic report that a maintainer, CI job, or coding agent can inspect before accepting a change. A passing report means that the enabled engines found no blocking findings; it is not a claim that every quality property has been proven.

## What is enforced

- **Role-aware discovery.** Inventory files are classified as source, test, generated, fixture, migration, configuration, documentation, vendor, or unknown before engines choose their inputs. Dependency and build-output directories are pruned by default. A user exclusion belongs only to the engine that owns it and remains visible as an advisory.
- **Structural budgets.** Tree-sitter metrics cover Rust, JavaScript, TypeScript, TSX, Python, and Go. File bytes/lines and per-function cyclomatic, cognitive, Halstead, ABC, parameter, statement, body-line, and nesting ceilings are configurable.
- **Anti-gaming policy.** Known suppression directives and project-forbidden tokens can be blocking findings on safety-checked roles. The current configuration has no inline approval or suppression exception channel.
- **Architectural boundaries.** Declarative path-scoped rules inspect import strings, call names, and tokens. This is a local rule scanner, not module resolution or type checking.
- **Clone debt.** Bounded token windows compare eligible files using verified normalized token sequences. Current clone findings carry a stable content fingerprint that does not include path or line numbers, so rename lineage can be matched during legacy adoption.
- **Evidence.** Enabled LCOV coverage and JSON mutation reports are required inputs. Empty, missing, unreadable, or malformed required evidence is a blocking finding. Disabled engines do not consume stale report files. Generated-artifact freshness is a separate required check when enabled.
- **Native mutation.** `hardgate mutate` runs a real unmutated baseline before bounded AST mutants, classifies outcomes, rejects a selection with no viable mutants, and restores source bytes after each mutant. It is separate from report ingestion.
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

The inventory also records `.css`, `.mdx`, `.sql`, `.json`, `.jsonc`, `.graphql`, `.gql`, `.snap`, `.toml`, `.yaml`, and `.yml`; these formats remain visible to classification and applicable safety rules but do not receive function metrics. Markdown (`.md`) is not a built-in inventory extension.

## Install

### npm and package-manager wrappers

The main npm package launches a prebuilt binary and does not require a Rust toolchain for ordinary use:

```sh
npm i -D @tech-byte-frontier/hardgate
npx hardgate check

pnpm add -D @tech-byte-frontier/hardgate
pnpm exec hardgate check --diff

yarn add -D @tech-byte-frontier/hardgate
yarn exec hardgate verify

bun add -d @tech-byte-frontier/hardgate
bunx --no-install hardgate scan src/index.ts
```

The wrapper publishes exactly six Unix optional packages and selects one by operating system, architecture, and (on Linux) libc:

| Target | Package |
| --- | --- |
| Linux x64, glibc | `hardgate-linux-x64` |
| Linux x64, musl | `hardgate-linux-x64-musl` |
| Linux arm64, glibc | `hardgate-linux-arm64` |
| Linux arm64, musl | `hardgate-linux-arm64-musl` |
| macOS x64 | `hardgate-darwin-x64` |
| macOS arm64 | `hardgate-darwin-arm64` |

If `HARDGATE_BINARY` is set, the launcher uses that binary first. Otherwise it checks the platform package selected by the matrix. On Linux it can fall back between the glibc and musl package when the first candidate is not usable, then to a development binary or `hardgate` on `PATH`. It never downloads a binary at runtime. Unsupported platforms fail closed.

### Cargo and source

```sh
cargo install hardgate --locked

git clone https://github.com/Tech-Byte-Frontier/hardgate.git
cd hardgate
cargo install --path . --locked
```

### Shell installer

The release installer supports the same six Unix targets. It accepts `HARDGATE_VERSION=latest`, `HARDGATE_VERSION=vX.Y.Z`, or `HARDGATE_VERSION=X.Y.Z`; `latest` is the default. `HARDGATE_INSTALL_DIR` selects the destination (otherwise `$HOME/.cargo/bin`). For every install it downloads the target archive and `SHA256SUMS`, requires one checksum entry for that archive, verifies the digest before extraction, reads `BUILD-METADATA.json`, and requires the installed binary to report the exact metadata version and full source commit (`hardgate VERSION (COMMIT)`). A version supplied without `v` is normalized to the release tag while the metadata is checked against the numeric version.

```sh
curl -fsSL https://raw.githubusercontent.com/Tech-Byte-Frontier/hardgate/main/scripts/install.sh | sh
curl -fsSL https://raw.githubusercontent.com/Tech-Byte-Frontier/hardgate/main/scripts/install.sh | HARDGATE_VERSION=vX.Y.Z sh
curl -fsSL https://raw.githubusercontent.com/Tech-Byte-Frontier/hardgate/main/scripts/install.sh | \
  HARDGATE_VERSION=X.Y.Z HARDGATE_INSTALL_DIR="$HOME/.local/bin" sh
```

There is no Windows or Homebrew installer in this contract.

## Initialize a policy

```sh
hardgate init --preset strict-agent
hardgate init --preset balanced
hardgate init --preset legacy-migration
```

With no `hardgate.toml`, Hardgate loads the `strict-agent` default bundle. That object is the same bundle rendered by `hardgate init --preset strict-agent`; in particular, coverage and mutation report policies are enabled with their configured floors (coverage also has its default `coverage/lcov.info` path), and the configured orchestration commands are present. A generated strict template therefore requires the project to provide a valid coverage report and mutation report path in TOML before `check` can pass; `verify --mutation-report <path>` can supply the mutation path for that command. If a project wants a structural-only starting point, it must explicitly set those sections to `enabled = false` (or choose `balanced`).

Preset behavior is deliberate:

- `strict-agent` uses the tight budgets and enables configured coverage/mutation evidence.
- `balanced` scales structural budgets and disables coverage/mutation report engines.
- `legacy-migration` scales structural budgets, disables coverage/mutation report engines, and enables the static legacy reference/merge-base ratchet. Its non-strict setting affects ordinary role-evidence fallback; explicitly enabled report, freshness, and reference failures remain blocking.
- `custom` uses values explicitly present in the file and deserialized defaults.

For a non-custom preset, TOML merging is presence-based. Only keys that are actually present in the file override the preset value; omitted keys retain the preset value. Explicit `false`, empty arrays, and other explicit values are not mistaken for omission.

## Commands and evidence boundaries

```sh
# Static engines plus enabled report and generated-freshness evidence.
hardgate check

# Changed/staged static scope. Clone matching uses a full repository index;
# enabled LCOV is attributed to changed executable source lines.
hardgate check --diff

# Add configured formatter, linter, and test commands.
hardgate check --all

# Opt in to configured dead-code analysis.
hardgate check --dead-code

# Full static verification plus enabled reports, freshness, and legacy ratchet.
hardgate verify

# Native baseline + AST-mutant execution (not report ingestion).
hardgate mutate --scoped src/lib.rs --test-cmd 'cargo test'

hardgate scan src/lib.rs
hardgate fmt --check
hardgate check --format agent
hardgate check --format json
```

The command contract is:

| Command | Runs | Does not run |
| --- | --- | --- |
| `check` | Static engines, enabled coverage/mutation reports, enabled generated freshness; optional configured dead code | Formatter/linter/test orchestration unless `--all`; native mutation |
| `check --diff` | Changed/staged static files, full clone index filtered to changed matches, changed executable LCOV, and (when enabled) full-tree legacy static ratchet | Native mutation; orchestration unless `--all` |
| `check --all` | Everything in `check` plus configured orchestration steps | Native mutation |
| `verify` | Full-tree static engines, enabled reports/freshness, and legacy static/dead-code ratchet | Orchestration and native mutation |
| `mutate` | Native unmutated baseline and bounded mutants | Coverage/mutation report ingestion |

Enabled required evidence fails closed when it is missing or empty. CLI `check` and `verify` retain an empty-discovery advisory and still run every enabled report, freshness, and legacy gate; the MCP `hardgate_check` surface rejects empty scopes/discovery instead of returning a successful empty report. Missing or empty Git evidence, coverage/mutation reports, generated freshness commands, and mutation outcomes are failures in the corresponding path. Disabled evidence engines do not inspect old report files. See [CLI reference and agent integration](docs/CLI_AND_INTEGRATION.md) for details.

## Roles, legacy adoption, and clones

The first-class role policies (`roles.source`, `roles.test`, `roles.generated`, `roles.fixture`, and `roles.migration`) are independent. Each can set severity (`error`, `warning`, or `ignore`), file/function ceilings, clone thresholds, and mutation-target eligibility. Built-ins classify generated files and fixtures before ordinary source conventions; ordered `[classification.rules]` may override built-ins except for vendor/build pruning.

Generated freshness is intentionally separate from file-budget exclusions. Excluding a generated path from byte/line checks does not disable its configured freshness command. Freshness failures remain current blocking evidence and are not grandfathered by legacy adoption.

When `[legacy].ratchet = true`, Hardgate resolves the configured reference and merge base, analyzes the baseline static snapshot (plus configured dead code), and compares it with the current static report. Existing non-worsened static debt can be grandfathered as advisories; new or worsened debt remains blocking. Git rename lineage and line-independent clone fingerprints preserve identity across safe renames. Retained findings include changed-file or changed-hunk context. Coverage, mutation, generated freshness, and configured orchestration findings remain current blocking evidence whenever their checks run and are never ratcheted.

## MCP and agent integration

`hardgate mcp` serves MCP over standard input/output. The static-only `hardgate_check(paths?, diff?)` tool routes through the same static gate as the CLI. It accepts optional path strings and a boolean `diff`; it does not run reports, freshness, orchestration, native mutation, or dead code. Invalid configuration, missing paths, empty scopes, unreadable files, parser failures, Git failures, and empty discovery return an explicit failed response instead of a successful empty report.

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

## Documentation

- [Vision and paradigm](docs/VISION_AND_PARADIGM.md)
- [Configuration specification](docs/CONFIGURATION_SPEC.md)
- [CLI reference and agent integration](docs/CLI_AND_INTEGRATION.md)
- [System architecture](docs/ARCHITECTURE.md)
- [Existing landscape](docs/EXISTING_LANDSCAPE.md)
- [X article](docs/X_ARTICLE.md)
- [API reference](https://docs.rs/hardgate)

## License

Dual-licensed under either [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
