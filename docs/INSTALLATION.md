# Installation

Hardgate 0.6 supports **Linux x64 GNU**. Cargo, direct release downloads, npm,
and pnpm provide the same CLI. This guide describes the 0.6 source contract;
use a published version from the [releases](https://github.com/Tech-Byte-Frontier/hardgate/releases)
when installing from a registry. A source version does not prove publication.

## Runtime requirements

The supported prebuilt baseline is Ubuntu 24.04 x64 with glibc 2.39 or newer.
Workload commands also require:

- Linux with Landlock ABI 3 or newer enabled for read-only child checks.
- A cgroup-v2 hierarchy with CPU, memory, swap, and task controllers available.
- systemd 254 or newer and an accessible user manager, unless the process already
  inherits the verified resource limits described in [resource limits](MUTATION_RESOURCES.md).
- The project's configured formatter, linter, tests, type checker, and evidence
  producers. Rust checks need Cargo, rustfmt, and Clippy; JS/TS checks need the
  selected project tools. Hardgate does not install these tools.

`init`, `config`, help, and version can run without the workload supervisor.
A version response alone does not verify `check` runtime support. Containers
without the required kernel facilities or user manager fail with setup guidance.

Linux ARM64, musl/Alpine, macOS, and Windows are deferred. The npm launcher
rejects unsupported hosts. Yarn and Bun are not tested installation channels.
Already published versions and artifacts remain available under their original
release contracts; 0.6 does not modify them.

## Cargo

Install a published version, then verify it in your project:

```sh
cargo install hardgate --locked
hardgate --version
hardgate check
```

For this source checkout:

```sh
cargo install --path . --locked
```

Use the stable toolchain declared in `rust-toolchain.toml` for source builds.
Cargo's executable directory is normally `$HOME/.cargo/bin`; `--root` and
`CARGO_INSTALL_ROOT` can select another prefix. Confirm `command -v hardgate`
resolves to the intended installation.

## npm and pnpm

The thin launcher needs Node.js 18 or newer and the matching
`hardgate-linux-x64` optional dependency. It has no postinstall or runtime
download. Install with optional dependencies enabled:

```sh
npm install --save-dev --save-exact @tech-byte-frontier/hardgate
npx --no-install hardgate check

pnpm add --save-dev --save-exact @tech-byte-frontier/hardgate
pnpm exec hardgate check
```

Commit the resulting lockfile. The exact wrapper and native package versions
must match. `HARDGATE_BINARY=/absolute/path/to/hardgate` explicitly selects a
locally supplied binary on a supported host.

Global installs use `npm install --global @tech-byte-frontier/hardgate` or
`pnpm add --global @tech-byte-frontier/hardgate`. For pnpm, run `pnpm setup` if
necessary, restart your shell, and put the directory from `pnpm bin --global`
on `PATH`. For npm, use the `bin` directory under `npm prefix --global`.

## Direct release downloads

Download `hardgate-linux-x64.tar.gz` and `SHA256SUMS` from the same signed
release. Verify the archive's checksum before extracting it:

```sh
sha256sum --check --ignore-missing SHA256SUMS
tar -xzf hardgate-linux-x64.tar.gz
./hardgate-linux-x64/hardgate --version
./hardgate-linux-x64/hardgate check
```

Run the checksum command in a directory containing the downloaded archive and
require a successful `hardgate-linux-x64.tar.gz: OK` result. The archive contains
`hardgate` and `BUILD-METADATA.json`, which identifies the version, source commit,
and Cargo target. GitHub provides checksum and SBOM attestations for the release.
Copy the verified executable into an executable directory on your `PATH` if
needed. There is no separate shell installer in 0.6.

## Upgrade and removal

Review the [changelog](../CHANGELOG.md) before upgrading. Use Cargo's
`--version VERSION --force`, or change the exact npm/pnpm dependency and commit
the lockfile. Verify `hardgate --version` and run `hardgate check` afterward.

Remove an installation with `cargo uninstall hardgate`,
`npm uninstall --save-dev @tech-byte-frontier/hardgate`, or
`pnpm remove --save-dev @tech-byte-frontier/hardgate`. Use the corresponding
`--global` option for a global npm/pnpm installation. For a direct download,
remove the exact executable you installed after checking its path.

Continue with [Getting started](GETTING_STARTED.md) for policy initialization,
or [Release recovery](RELEASE_RECOVERY.md) for maintainer recovery procedures.
