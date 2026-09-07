# Maintainer guide

Work from the live checkout and preserve unrelated changes. Describe public
behavior, focused validation, and material limitations in pull requests. Keep
CLI/configuration migrations, package metadata, installation docs, and curated
release notes aligned with the implementation.

## Validation

Use the stable toolchain in `rust-toolchain.toml` for formatting, Clippy,
tests, and product builds. The coverage script pins a separate nightly for
real LLVM branch coverage. Rust 1.90 is tested separately as the MSRV with the
locked graph; keep that CI job aligned with Cargo.toml. On Linux, serialize Rust builds, tests, coverage, mutation,
and installed Rust consumer checks under `scripts/with-resource-limits.sh`.
Track disposable artifacts and verify input restoration.

PR CI always requires formatting, linting, all-target/all-feature tests,
RustSec audit, report/integration contracts, and the self/evidence gate. The
self-gate replaces native mutation with an explicit single budget-engine
cargo-mutants sample; that sample is not repository-wide mutation coverage.
Distribution-sensitive PR changes also run crate/package/ABI/SBOM and actual
packed-install checks. Main CI always runs those distribution checks and builds
native release binaries for Linux x64/ARM64 and macOS Intel/Apple Silicon.
The native matrix exercises real packed npm installs and local static analysis. The stable `CI quality aggregate`
rejects failed, cancelled, or skipped required jobs. Conditional packaging
steps do not remove required jobs from the aggregate.

## Release preparation

Local analysis supports macOS and Linux through Cargo, direct
downloads, npm, and pnpm. The wrapper and every native package must match Cargo.toml, Cargo.lock, and
the root package version. `scripts/release-platforms.mjs` defines the supported
distribution map. Linux prebuilt artifacts must fit the glibc 2.39 baseline; project-tool
execution needs the kernel and resource facilities in [Installation](INSTALLATION.md).

A new signed annotated `vX.Y.Z` tag must identify the exact main tip with
successful CI. Validate the tag using `.github/release-allowed-signers` and
preserve immutable versions. Inspect public registries and GitHub assets before
publication; an existing version requires byte verification and reuse, never
overwriting or republishing.

The release workflow has six stages: tag validation, packaging, publication,
exact consumers, promotion, and default consumers/completion. Packaging reuses
the exact main CI artifact by run ID, artifact ID, source SHA, and digest.
It creates four reproducible archives, checksums, and SBOM and tests actual npm
and pnpm installs. It never rebuilds the shared native binary. Publication
attests the verified bundle, publishes only missing artifacts, and preserves
partial receipts. Cargo installation necessarily builds from the verified
crate; its installed `hardgate check` behavior is tested separately.

All seven receipt channels must reach exact-consumer verification before
promotion, and default-consumer verification before completion. npm and pnpm
project/global installs, Cargo, and direct downloads on Linux exercise real
`hardgate check` and test-failure propagation with unchanged inputs. Each other native platform also installs the exact/default registry package,
checks binary identity, and exercises static analysis before its receipt advances.
No full repository gate is repeated during release after the exact-source CI gate.

## Recovery and external state

Use **Re-run failed jobs** to retain the successful immutable bundle checkpoint.
If the workflow needs repair, use its reviewed same-tag `resume_run_id` path.
The signed payload and CI-validated recovery tooling are separate identities;
launcher compatibility and all artifact bytes remain checked. Existing
registry versions are accepted only after matching manifests, platform
constraints, executable mode, and binary bytes are proven.

Bundles last 30 days; attempt-specific receipts last 90 days. Missing or
expired evidence is not permission to rebuild different bytes or move a tag.
See [Release recovery](RELEASE_RECOVERY.md) for exact state and retention rules,
and [Publisher setup](PUBLISHER_SETUP.md) for scoped authentication.

Repository-rule proposals remain review-only. Local workflow tests do not
prove remote deployment, publisher configuration, signing availability, or a
successful public release. External activation and publication must remain
within the authorized task scope.
