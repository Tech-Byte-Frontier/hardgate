# Installation

Local analysis supports **macOS and Linux**. Cargo, direct release
downloads, npm, and pnpm provide the same CLI. This guide describes the current
source contract; use a published version from the
[releases](https://github.com/Tech-Byte-Frontier/hardgate/releases) when installing
from a registry. Existing releases retain their original platform contracts.

## Feature requirements

`check`, `fmt`, configured tests/type checks, generated freshness, and static
analysis run natively on macOS and Linux with the configured project tools
installed. Checks use disposable copies and verify source inputs before and
after execution. Timeouts, process-group cleanup, bounded output, and conservative
worker defaults remain active. Native execution does not enforce OS CPU/memory
limits or filesystem write containment; the CLI and saved check report say so.
Only run trusted project commands in this mode.

Policies that need enforced containment can require it explicitly:

```toml
[orchestration]
require_isolation = true
```

This setting and all `evidence` producers require:

- Linux cgroup v2 with CPU, memory, swap, and task controllers.
- systemd 254+ with an accessible user manager, unless verified limits are inherited.
- Landlock ABI 3+ for protected child checks.

Required isolation never falls back to native execution. Unsupported requirements
fail with exit 2 before project tools start. Ordinary checks also reuse verified
inherited Linux containment when available. Static commands and policy-only
checks without generated freshness need no tool execution. Policy-only checks
remain partial; missing required evidence never becomes a passing acceptance.
See [resource limits](MUTATION_RESOURCES.md) for Linux execution setup.

## Native packages

| Host | Native npm package / archive name |
| --- | --- |
| Linux x64, glibc 2.39+ | `hardgate-linux-x64` |
| Linux ARM64, glibc 2.39+ | `hardgate-linux-arm64` |
| macOS Intel | `hardgate-darwin-x64` |
| macOS Apple Silicon | `hardgate-darwin-arm64` |

Each archive is named `<package>.tar.gz` and contains `hardgate` plus
`BUILD-METADATA.json`. The native CI matrix builds and tests these platforms.
Windows and musl/Alpine are not supported. Cargo source builds have no artificial
target allowlist; untested targets carry no compatibility guarantee. Yarn and
Bun are not tested installation channels.

## Cargo

Install a published version, then verify it in your project:

```sh
cargo install hardgate --locked
hardgate --version
hardgate check --checks policy
```

For this source checkout:

```sh
cargo install --path . --locked
```

The minimum supported Rust version is **1.90** with the locked dependencies.
CI tests that compiler separately from the **1.98.1** development/release pin in
`rust-toolchain.toml`; the pin is not the installation minimum. The dependency
Tree-sitter 0.27 declares Rust 1.90, and compatibility tests cover the locked
graph and local analysis. Use `cargo +1.90.0 install --path . --locked` to exercise
the minimum compiler explicitly.
Cargo's executable directory is normally `$HOME/.cargo/bin`; `--root` and
`CARGO_INSTALL_ROOT` can select another prefix. Confirm `command -v hardgate`
resolves to the intended installation.

## npm and pnpm

The thin launcher needs Node.js 18 or newer and the matching native optional
dependency from the table above. **Rust is not required.** It has no postinstall or runtime
download. Install with optional dependencies enabled:

```sh
npm install --save-dev --save-exact @tech-byte-frontier/hardgate
npx --no-install hardgate check --checks policy

pnpm add --save-dev --save-exact @tech-byte-frontier/hardgate
pnpm exec hardgate check --checks policy
```

Commit the resulting lockfile. The exact wrapper and native package versions
must match. `HARDGATE_BINARY=/absolute/path/to/hardgate` explicitly selects a
locally supplied binary on a supported host.

Global installs use `npm install --global @tech-byte-frontier/hardgate` or
`pnpm add --global @tech-byte-frontier/hardgate`. For pnpm, run `pnpm setup` if
necessary, restart your shell, and put the directory from `pnpm bin --global`
on `PATH`. For npm, use the `bin` directory under `npm prefix --global`.

### GitHub Packages mirror

The repository includes a workflow to mirror the npm wrapper into GitHub
Packages. Confirm the desired version appears in the repository's **Packages**
section before using this channel; checked-in workflow code does not prove it
has been published or made public.

GitHub requires authentication even for public npm packages. Use a personal
access token (classic) with `read:packages` when prompted for a password:

```sh
npm login --scope=@tech-byte-frontier --auth-type=legacy --registry=https://npm.pkg.github.com
npm install --save-dev --save-exact @tech-byte-frontier/hardgate --registry=https://registry.npmjs.org
npx --no-install hardgate check --checks policy
```

The login configures `@tech-byte-frontier` to use GitHub Packages. Keep the
default registry on npmjs.org so the matching unscoped native
dependency can be installed. The same scope mapping works with pnpm. Commit
the lockfile, but never commit a registry token. Normal npm installation above
remains available without GitHub authentication.

See [GitHub's npm registry guide](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-npm-registry)
for token and registry configuration.

## Direct release downloads

Download the archive for your host and `SHA256SUMS` from the same signed
release. The following Linux example uses `hardgate-linux-x64.tar.gz`. Verify the archive's checksum before extracting it:

```sh
sha256sum --check --ignore-missing SHA256SUMS
tar -xzf hardgate-linux-x64.tar.gz
./hardgate-linux-x64/hardgate --version
./hardgate-linux-x64/hardgate check --checks policy
```

Run the checksum command in a directory containing the downloaded archive and
require a successful `hardgate-linux-x64.tar.gz: OK` result. The archive contains
the native executable and `BUILD-METADATA.json`, which identifies the version, source commit,
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
