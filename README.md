# Hardgate

**Deterministic quality gates, structural budgets, and anti-gaming checks for agent-assisted software.**

[![Crates.io](https://img.shields.io/crates/v/hardgate.svg)](https://crates.io/crates/hardgate)
[![Documentation](https://docs.rs/hardgate/badge.svg)](https://docs.rs/hardgate)
[![License](https://img.shields.io/crates/l/hardgate.svg)](https://github.com/Tech-Byte-Frontier/hardgate#license)

Hardgate is a local Rust CLI. It turns repository policy into a deterministic report that an engineer or an agent can inspect before accepting a change. The gate is deliberately about evidence and boundaries, not a promise that a test command alone proves correctness.

## What is enforced

- **Role-aware discovery.** Source, test, fixture, generated, migration, configuration, documentation, vendor, and unknown files are classified before engines choose their inputs. Dependency and build-output directories are pruned by default; user exclusions remain visible as advisories.
- **Structural budgets.** Tree-sitter metrics cover Rust, JavaScript, TypeScript, TSX, Python, and Go. Configure physical bytes/lines and per-function cyclomatic, cognitive, Halstead, ABC, parameter, statement, body-line, and nesting ceilings.
- **Anti-gaming policy.** When enabled, known suppression directives and project-forbidden tokens fail safety-checked files. There is no inline exception mechanism in the current configuration.
- **Architectural boundaries.** Declarative rules inspect source lines for disallowed imports, calls, and tokens. This is a local rule scanner, not global module analysis.
- **Clone debt.** A bounded rolling hash over normalized lexical token streams finds repeated spans across eligible source, test, and fixture files. Thresholds and per-engine exclusions are configurable.
- **Optional evidence.** Enabled coverage policies read LCOV and can enforce global line/function/branch floors, CRAP ceilings, and critical-path line coverage. Enabled mutation policies evaluate Stryker, cargo-mutants, or generic JSON outcome reports. Missing or malformed required evidence is a finding in strict mode; disabled engines ignore stale reports.
- **Native mutation.** `hardgate mutate` is a separate AST mutation run over classified production sources. It runs an unmutated baseline first, rejects a run with no viable mutants, reports killed/survived/timeout/compile/runner/equivalent/unviable outcomes, and verifies source restoration after every mutant.
- **Local integration.** `check --all` can run configured formatter, linter, and test commands. `fmt` can run the configured formatter. `--format agent`, JSON, compact, and summary output make results consumable by tools without hiding the terminal report.

### Supported source and inventory extensions

Tree-sitter parsing currently covers these six parser targets:

| Family | Extensions |
| --- | --- |
| Rust | `.rs` |
| JavaScript | `.js`, `.jsx`, `.mjs`, `.cjs` |
| TypeScript | `.ts`, `.mts`, `.cts` |
| TSX | `.tsx` |
| Python | `.py` |
| Go | `.go` |

The inventory also records `.css`, `.mdx`, `.sql`, `.json`, `.jsonc`, `.graphql`, `.gql`, `.snap`, `.toml`, `.yaml`, and `.yml`; these formats do not receive a Tree-sitter complexity pass in this revision. Markdown documentation (`.md`) is not part of the built-in inventory.

## Quickstart

### Install

```sh
# npm wrapper (prebuilt binary; no Rust toolchain required)
npm i -D @tech-byte-frontier/hardgate
npx hardgate check

# Cargo
cargo install hardgate

# Source checkout
git clone https://github.com/Tech-Byte-Frontier/hardgate.git
cd hardgate
cargo install --path . --locked
```

The npm wrapper publishes six Unix optional packages and selects one by OS, architecture, and (on Linux) libc:

| Target | Package |
| --- | --- |
| Linux x64, glibc | `hardgate-linux-x64` |
| Linux x64, musl | `hardgate-linux-x64-musl` |
| Linux arm64, glibc | `hardgate-linux-arm64` |
| Linux arm64, musl | `hardgate-linux-arm64-musl` |
| macOS x64 | `hardgate-darwin-x64` |
| macOS arm64 | `hardgate-darwin-arm64` |

The shell installer currently downloads a release tarball. The stabilization target is to make it use this same six-package matrix, verify the release `SHA256SUMS` before extraction, and accept `HARDGATE_VERSION` (`latest` or a release tag) plus `HARDGATE_INSTALL_DIR`. The release script still needs that checksum/platform alignment before this target can be described as shipped.

### Initialize policy

```sh
hardgate init --preset strict-agent
hardgate init --preset balanced
hardgate init --preset legacy-migration
```

`strict-agent` uses the tightest structural budgets. `balanced` scales those budgets. `legacy-migration` currently uses scaled budgets and non-strict evidence handling; it does **not** compare against a merge-base baseline or ratchet debt in this revision. A merge-base baseline/ratchet is a stabilization target, not an enabled feature.

### Common commands

```sh
# Static engines over discovered files. Enabled report policies are evaluated too.
hardgate check

# Static engines over git-modified/staged files. Clone matching uses a full
# repository index and reports only matches touching changed files.
hardgate check --diff

# Add configured formatter/linter/test commands to the static run.
hardgate check --all

# Explicitly request dead-code analysis (or enable it in hardgate.toml).
hardgate check --dead-code

# Evaluate static engines plus enabled LCOV and mutation-report evidence.
hardgate verify

# Execute native AST mutation testing; this does not invoke Stryker.
hardgate mutate --scoped src/lib.rs --test-cmd 'cargo test'

# Inspect one file, format through [orchestration], or emit agent-friendly data.
hardgate scan src/lib.rs
hardgate fmt --check
hardgate check --format agent
hardgate check --format json
```

`check` and `verify` do not run a test suite or native mutation pass by themselves. `check --all` runs only commands configured under `[orchestration]`; it does not invent a test command. `verify` ingests report files when the corresponding policy is enabled. `mutate` is the command that executes baselines and mutants. A strict run cannot pass while required configured evidence is missing or unreadable.

## Agent integration

Use `--format agent` for compact, actionable Markdown diagnostics. JSON, compact, and summary modes are available on `check`, `scan`, and `verify`; `mutate` supports terminal, agent, and JSON output.

`hardgate mcp` is an MCP server over standard input/output. Register the command with an MCP client:

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

The server exposes these static-analysis tools:

- `hardgate_check` — check the repository or supplied paths;
- `hardgate_scan_file` — inspect one file;
- `hardgate_get_metrics` — return metrics for a named function.

The MCP surface does not run orchestration, coverage, mutation, or dead-code commands.

## Configuration

`hardgate.toml` is optional. Without a file, Hardgate loads the strict-agent defaults. `hardgate init` writes an explicit template with coverage, mutation, orchestration, and dead-code commands disabled until a project supplies those inputs.

```toml
[gate]
name = "project"
preset = "strict-agent"
strict = true
enforce_classified_sources = false

[budgets.files]
max_bytes = 32768

[budgets.files.max_lines]
rs = 499
ts = 400
tsx = 400
js = 400
default = 350

[budgets.functions]
max_cyclomatic = 10
max_cognitive = 15
max_halstead_difficulty = 80.0
max_parameters = 4
max_lines = 80
max_nesting_depth = 4

[anti_gaming]
disallow_suppressions = true

[clones]
enabled = true
min_lines = 5
min_tokens = 50

[coverage]
enabled = false
report = "coverage/lcov.info"
min_line_percent = 95.0
max_crap_score = 25.0

[mutation]
enabled = false
min_score = 85.0
timeout_secs = 10
max_mutants = 30
```

See the [configuration specification](docs/CONFIGURATION_SPEC.md) for every field and the [CLI reference](docs/CLI_AND_INTEGRATION.md) for command semantics.

## Design notes

Hardgate's value is a deterministic local policy: structural budgets that are hard to game, explicit anti-suppression checks, architecture rules close to the code, bounded clone detection, and evidence that is either present and evaluated or explicitly disabled. It is complementary to language linters, coverage providers, mutation runners, and hosted quality dashboards; it is not a replacement for those tools' language-specific semantics.

Planned stabilization work is intentionally not represented as current behavior: merge-base baseline/ratchet evaluation, diff coverage and new-clone fingerprints, broader consumer fixtures, and release-script checksum/publication checks require their respective implementations to land first.

## Documentation

- [Vision & Paradigm](docs/VISION_AND_PARADIGM.md)
- [Configuration Specification](docs/CONFIGURATION_SPEC.md)
- [CLI Reference & Agent Integration](docs/CLI_AND_INTEGRATION.md)
- [Existing Landscape](docs/EXISTING_LANDSCAPE.md)
- [System Architecture](docs/ARCHITECTURE.md)
- [API reference](https://docs.rs/hardgate)

## License

Dual-licensed under either [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
