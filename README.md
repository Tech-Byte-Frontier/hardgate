# Hardgate

[![Crates.io](https://img.shields.io/crates/v/hardgate.svg)](https://crates.io/crates/hardgate)
[![Documentation](https://docs.rs/hardgate/badge.svg)](https://docs.rs/hardgate)
[![CI](https://github.com/Tech-Byte-Frontier/hardgate/actions/workflows/ci.yml/badge.svg)](https://github.com/Tech-Byte-Frontier/hardgate/actions/workflows/ci.yml)
[![License](https://img.shields.io/crates/l/hardgate.svg)](https://github.com/Tech-Byte-Frontier/hardgate/blob/main/LICENSE-MIT)

Hardgate is a local Rust CLI for deterministic quality gates, structural
budgets, and anti-gaming checks in agent-assisted repositories. It turns
repository policy and its required evidence into a report that maintainers,
CI jobs, and coding agents can inspect before accepting a change.

A passing report means that the enabled engines found no blocking findings. It
does not claim that every possible quality property was proven.

## Quick start

Install the latest released Cargo CLI, then initialize a structural policy in
the project you want to check:

```sh
cargo install hardgate --locked
cd /path/to/your/project
hardgate init
hardgate check
```

`balanced` is a structural starting point. Initialization does not install
project tools or execute project commands. An existing project may still fail
its first check because its source roles, budgets, commands, or evidence need
project-specific decisions. See [Getting started](docs/GETTING_STARTED.md)
for previews, diagnostics, and a small refactor walkthrough.

This repository also contains an unreleased source checkout. To try that
checkout, run `cargo install --path . --locked` from its root; do not use its
source version as an npm or registry install target before a release.

## What Hardgate checks

- **Role-aware discovery:** files receive source, test, generated, fixture,
  migration, configuration, documentation, vendor, or unknown roles before
  engines choose their inputs.
- **Structural budgets:** configurable file and function budgets use Tree-sitter
  metrics for Rust, JavaScript, TypeScript/TSX, Python, and Go.
- **Anti-gaming and architecture:** suppression, forbidden-token, and
  declarative path-scoped import, call, and token rules can block a change.
- **Clone debt:** bounded normalized-token comparisons produce stable,
  path-independent clone fingerprints.
- **Evidence:** enabled LCOV, mutation-report, and generated-freshness checks
  fail closed when required inputs are missing, empty, unreadable, or malformed.
- **Native mutation:** an optional baseline-plus-mutant workflow runs in a
  private workspace and verifies source restoration on Linux and macOS.
- **Orchestration:** `check --all` runs only the formatter, linter, and test
  commands configured by the repository.

Hardgate inventories additional text and data formats for classification and
safety rules. It does not claim compiler or type-checker analysis, global
module resolution, or a hosted quality dashboard.

## Command boundaries

| Command | Purpose |
| --- | --- |
| `check` | Static engines plus enabled evidence and freshness checks |
| `check --diff` | Changed/staged static scope and changed executable-line coverage |
| `check --all` | `check` plus configured formatter, linter, and test commands |
| `verify` | Full static/dead-code scope plus configured evidence and ratchet checks |
| `mutate` | Native baseline and bounded AST mutants when native mutation is enabled |
| `mcp` | Static check, file scan, and metrics tools over stdio |

These commands keep static analysis, report evaluation, orchestration, and
native mutation separate. See the [CLI reference](docs/CLI_AND_INTEGRATION.md)
for scope, evidence, exit status, and agent integration details.

## Documentation

| Need | Guide |
| --- | --- |
| Install Cargo, npm, pnpm, Yarn, Bun, or shell channels | [Installation](docs/INSTALLATION.md) |
| Initialize a policy and follow the first check loop | [Getting started](docs/GETTING_STARTED.md) |
| Command behavior and agent/MCP integration | [CLI reference](docs/CLI_AND_INTEGRATION.md) |
| Presets, roles, budgets, evidence, and classification | [Configuration](docs/CONFIGURATION_SPEC.md) |
| Internal components and execution boundaries | [Architecture](docs/ARCHITECTURE.md) |
| Native mutation resources and limits | [Mutation resources](docs/MUTATION_RESOURCES.md) |
| JSON reports and execution evidence | [Report schema](docs/REPORT_SCHEMA.md) |
| Stable diagnostic rule IDs | [Diagnostic rules](docs/DIAGNOSTIC_RULES.md) |
| Comparison with adjacent tools | [Existing landscape](docs/EXISTING_LANDSCAPE.md) |
| Product direction | [Vision and paradigm](docs/VISION_AND_PARADIGM.md) |

For release operations, see the [maintainer guide](docs/MAINTAINERS.md), [release
recovery runbook](docs/RELEASE_RECOVERY.md), and [publisher setup](docs/PUBLISHER_SETUP.md).
The [Rust API reference](https://docs.rs/hardgate) is generated from the crate.

## Contributing and license

Read [Contributing](CONTRIBUTING.md) and the [Code of Conduct](CODE_OF_CONDUCT.md)
before opening an issue or pull request. See the [security policy](SECURITY.md)
for private vulnerability reports.
See the [changelog](CHANGELOG.md) for user-facing behavior changes.

Hardgate is available under either the [Apache License 2.0](LICENSE-APACHE) or
the [MIT License](LICENSE-MIT), at your option.
