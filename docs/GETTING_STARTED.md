# Getting started

This guide follows the current source CLI. It assumes that `hardgate` is on
`PATH`; if you installed a published channel, see the
[installation guide](INSTALLATION.md). A source checkout may include behavior
that has not shipped in a release:

```sh
cargo install --path . --locked
```

Run the commands below from the root of the project you want Hardgate to
inspect.

## 1. Initialize a policy

`init` defaults to balanced: structural feedback before adding coverage and
mutation-report producers. Select `--preset strict-agent` explicitly when those
required evidence producers are ready:

```sh
hardgate init
hardgate config
```

Initialization inspects project-root manifests and existing scripts to describe
possible commands. It does not install dependencies or execute project tools.
It creates `hardgate.toml` only when that path is free; an existing file,
directory, or symlink is left untouched. Read the completion summary for
detected ecosystems, enabled engines, missing setup, and the next command.

An existing repository may fail its first check. That is expected when its
source roles, budgets, commands, evidence, or parser classifications need
project-specific decisions. Treat the first report as a policy decision point,
not as proof that the repository is defective.

### Preview without writing

Use `--preview` when you want valid generated TOML without changing the
working tree:

```sh
hardgate init --preset balanced --preview > /tmp/hardgate.toml
hardgate init --preset balanced --preview --full > /tmp/hardgate-effective.toml
hardgate config --format toml
```

Preview TOML goes to stdout and the setup summary goes to stderr, so redirecting
stdout produces a usable policy file. `--full` renders the effective policy
instead of the compact template. Use the [configuration
reference](../docs/CONFIGURATION_SPEC.md) for the fields and preset behavior.

### Choose a preset

| Preset | Starting point |
| --- | --- |
| `balanced` | Structural budgets and safety checks without required coverage or mutation reports |
| `strict-agent` | Strict structural policy with required LCOV and mutation-report evidence |
| `legacy-migration` | Structural adoption with a trusted reference and merge base |
| `custom` | Ordinary deserialized defaults for an explicitly authored policy |

Strict-agent retains 95% line/function coverage, 90% branch coverage, and an
85% mutation floor. Balanced does not claim test adequacy. Running without a
policy still uses strict-agent; initialize deliberately rather than relying on
an implicit adoption mode.

Tests remain analyzed. Their size and duplication findings are advisories;
complexity and safety findings still block. Source clones below the blocking
minimum also remain visible as advisories. The [policy reference](CONFIGURATION_SPEC.md)
lists detection and blocking thresholds and explicit enforcement overrides.

### Adopt an existing codebase

For a repository with historical debt, initialize explicitly with:

```sh
hardgate init --preset legacy-migration
hardgate config
hardgate check --diff --json --summary
```

The default reference is `origin/main`. Fetch it if needed, or configure
`[legacy].reference_branch` to a trusted reference with a merge base. Init never
fetches or rewrites an existing policy; for an existing `hardgate.toml`, review
`init --preset legacy-migration --preview` and intentionally merge the policy.

The ratchet retains old non-worsened static debt as advisories and blocks new
or worsened error-severity findings. Its verdict states the reference and
merge base and does not certify a debt-free repository. An unusable reference
is a blocking evidence failure. Enabled coverage, mutation, generated freshness,
and orchestration remain current requirements. With `--diff`, enabled coverage
uses changed executable source lines; static comparison uses the selected
current scope against the reference snapshot. A full `check` without a ratchet
is the separate assessment of all configured debt.

No preset decides project-specific commands for every ecosystem. Mixed
repositories, nested packages, or ambiguous scripts need explicit commands or
initialization from the relevant project root.

## 2. Run the first check

Start with the fast structural and evidence-aware report:

```sh
hardgate check
hardgate check --format compact
```

Use agent or JSON output when another tool will consume the result:

```sh
hardgate check --format agent
hardgate check --format json
```

A passing report means the enabled engines found no blocking findings. It does
not prove that disabled engines ran, that a project command was executed, or
that every quality property was checked. Missing, empty, unreadable, or
malformed required evidence is a blocking result when its engine is enabled.

When the policy is configured, `check --all` adds only its formatter, linter,
and test commands:

```sh
hardgate check --all
```

Hardgate does not invent commands or install project tools. Review the
configured commands with `hardgate config` before enabling orchestration.

## 3. See a pass, a diagnostic, and a refactor

The following small Rust fixture demonstrates the structural loop without
depending on the source repository:

```sh
smoke_dir="$(mktemp -d)"
mkdir -p "$smoke_dir/src"
cat > "$smoke_dir/Cargo.toml" <<'FIXTURE_TOML'
[package]
name = "hardgate-smoke"
version = "0.1.0"
edition = "2021"
FIXTURE_TOML
cat > "$smoke_dir/src/lib.rs" <<'FIXTURE_RUST'
pub fn add(left: u32, right: u32) -> u32 {
    left + right
}
FIXTURE_RUST
cd "$smoke_dir"
hardgate init --preset balanced
hardgate config
hardgate check --format compact
```

The balanced policy should pass this fixture. To create a deterministic
parameter-budget diagnostic, replace `src/lib.rs` with a seven-parameter
function:

```rust
pub fn sum(a: u32, b: u32, c: u32, d: u32, e: u32, f: u32, g: u32) -> u32 {
    a + b + c + d + e + f + g
}
```

Run the compact check again. The balanced preset's default parameter ceiling is
six, so the report should identify the seventh parameter as a budget finding.
The report is the useful artifact: it includes the rule, location, and
remediation context instead of requiring a score interpretation.

Refactor the function by making the data explicit:

```rust
pub fn sum(values: [u32; 7]) -> u32 {
    values.into_iter().sum()
}
```

Run `hardgate check --format compact` again. The fixture should return to a
passing structural gate. This example is intentionally bounded; it does not
claim that an arbitrary existing repository will pass without policy and
evidence decisions.

## 4. Follow a finding

Use the command that matches the question:

| Question | Command |
| --- | --- |
| What does the current policy evaluate? | `hardgate config --format toml` |
| What changed in the worktree? | `hardgate check --diff` |
| What does a full configured evidence run say? | `hardgate verify` |
| What does native mutation find? | `hardgate mutate --scoped src/lib.rs --test-cmd 'cargo test'` |
| What does one file's static report say? | `hardgate scan src/lib.rs` |

`check --diff` selects changed or staged files for static analysis and
intersects coverage with changed executable lines. Clone matching still uses
the eligible repository index. Explicit existing paths add to static and clone
selection.

`verify` evaluates the full static/dead-code scope plus configured evidence,
freshness, and legacy-ratchet checks. It does not run formatter, linter, test,
or native mutation commands. `mutate` is a separate native baseline-plus-
mutants workflow; mutation-report ingestion is a different evidence engine.

If a report is incomplete, inspect the effective policy and its producer paths
before changing thresholds:

```sh
hardgate config --format json
hardgate check --format agent
```

Under strict-agent, LCOV and mutation-report paths must point to real,
non-empty, parseable reports. Under balanced, those report engines are
disabled by default. A parser-unsupported file that remains in a source or
migration role can produce `unsupported-source`; resolve it with an explicit,
truthful classification or role policy rather than pretending it has function
metrics.

## 5. Continue with the references

- [CLI reference and agent integration](../docs/CLI_AND_INTEGRATION.md) explains
  command scope, exit status, MCP, JavaScript resolution, and native mutation.
- [Configuration specification](../docs/CONFIGURATION_SPEC.md) defines presets,
  roles, budgets, evidence, freshness, and classification.
- [Architecture](../docs/ARCHITECTURE.md) describes execution boundaries and
  data flow.
- [Report schema](../docs/REPORT_SCHEMA.md) defines machine-readable status,
  execution evidence, diagnostics, and compatibility.
- [Diagnostic rules](../docs/DIAGNOSTIC_RULES.md) lists stable `HG-*` IDs.
- [Existing landscape](../docs/EXISTING_LANDSCAPE.md) explains complementary
  tools and current platform boundaries.
