# Contributing to Hardgate

Keep changes focused, reproducible, and easy to verify. Start with an issue when
the behavior or scope is unclear; a pull request should explain the change and
the checks that support it.

## Toolchains

Use the repository pins:

- Rust `1.98.1`, from `rust-toolchain.toml`, with `rustfmt`, `clippy`, and
  `llvm-tools-preview`.
- Node `26.8.1`, from `.nvmrc`.
- CI also pins npm `12.0.2` and pnpm `11.25.0`.
- Coverage uses nightly Rust `nightly-2026-09-04`, `cargo-llvm-cov` `0.9.0`,
  and `cargo-audit` `0.22.2`.

Keep Rust builds, coverage, and mutation runs serialized. They are resource
heavy and concurrent runs can observe or overwrite temporary build state.

## Validation

Run the narrowest relevant checks while iterating. Examples are a targeted
`cargo test <filter> --locked`, one relevant `node tests/<file>.mjs`,
`cargo fmt --all --check`, and `hardgate check --diff` when the change affects
the CLI policy or analyzed source. Record the exact command and runtime in the
pull request.

Before requesting review, run the repository-wide checks that apply. The Rust
CI job runs:

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
scripts/dependency-audit.sh
cargo publish --dry-run --locked
cargo build --locked --release
```

The package and release checks include:

```sh
node scripts/sync-npm-version.mjs --check
node scripts/check-npm-quality.mjs
node tests/npm-wrapper.test.mjs
node tests/npm-wrapper-regression.test.mjs
node tests/consumer_matrix.mjs
node tests/release_contract.sbom.test.mjs
node tests/release_contract.test.mjs
node tests/release_contract.package.test.mjs
node tests/release_contract.abi.test.mjs
```

The complete configured evidence gate is `scripts/self-gate.sh`. It covers
static policy and specialist lint checks, pinned coverage, a real mutation sample, and the
consumer matrix. Run it only with its required pinned tools and a repository
binary available at `target/release/hardgate` or through `HARDGATE_BINARY`.

For package or release changes, validate exact identity. Cargo is the version
source; `sync-npm-version.mjs --check` and `check-npm-quality.mjs` must pass.
Freshly staged binaries and archives must report `hardgate VERSION (FULL_COMMIT)`
and match their target/package metadata. A filename or manifest version alone
is not sufficient.

## Mutation and fixtures

Specialist mutation runs in a private copy of the current working files, including
dirty and untracked inputs. Dependencies are copied; `.git` and `target` are
omitted. SIGINT and SIGTERM stop owned children and remove the copy. SIGKILL
can leave that copy behind, but mutants never replace the original sources.
Keep expensive builds and mutation jobs serialized, retain their evidence,
then remove task-owned targets and completed worktrees. See the command
reference for symlink rules and the trusted test-command boundary.

When fixing a bug, add the smallest deterministic fixture or test that shows
the failure. Keep the reproduction self-contained, state the exact command,
toolchain, expected result, and actual result, and remove credentials or
machine-specific paths. Follow the nearest existing convention under `tests/`
and `tests/support/` rather than adding a large general-purpose fixture.

Do not make a failing check green by lowering a threshold, adding an exclusion
or suppression, disabling required evidence, or weakening commit/tag signing.
If the configured signer is unavailable, leave the change staged and report
that blocker rather than bypassing the signing policy.
