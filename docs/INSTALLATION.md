# Installation

Choose the channel that matches how the project runs Hardgate. The Cargo CLI is
the primary installation. The npm wrapper launches a prebuilt Rust binary for
JavaScript package-manager workflows.

The pinned examples in this guide describe the repository snapshot observed
while writing: Cargo/GitHub release `v0.5.0` and published npm wrapper
`0.4.2`. Verify the registry or release page before copying a pin. The source
checkout may contain unreleased changes; its source version is not an npm
install target.

## Cargo CLI

Install the latest published CLI:

```sh
cargo install hardgate --locked
hardgate --version
```

For the released snapshot documented above, the equivalent exact install is:

```sh
cargo install hardgate --version 0.5.0 --locked
```

Cargo installs the executable under the selected install root's `bin`
directory. `--root` takes precedence, followed by `CARGO_INSTALL_ROOT`,
Cargo's `install.root` setting, and `CARGO_HOME` (normally
`$HOME/.cargo`). With rustup, load the standard path when needed:

```sh
. "$HOME/.cargo/env"
command -v hardgate
hardgate --version
```

### Current source checkout

Use this flow when you need behavior in an unreleased checkout:

```sh
git clone https://github.com/Tech-Byte-Frontier/hardgate.git
cd hardgate
cargo install --path . --locked
```

This installs the checkout you selected and does not claim that its version is
published to crates.io or npm. For the first policy and check loop, continue
with [Getting started](GETTING_STARTED.md).

## npm, pnpm, Yarn, and Bun

The published npm wrapper in the documented snapshot is `0.4.2`. It requires
Node.js 18 or newer. Use an exact project dependency and the package manager
used by the project:

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

The source tree's npm manifests may carry the Cargo release number before that
npm wrapper version is published. Use the exact published wrapper version above
or use the current source checkout; do not install an unpublished npm version.

### Global npm or pnpm use

Global installs expose the same `hardgate` command from any project:

```sh
npm install --global @tech-byte-frontier/hardgate@0.4.2
hardgate --version
```

For pnpm, first ensure its global executable directory is configured:

```sh
pnpm setup
pnpm_global_bin="$(pnpm bin --global)"
export PATH="$pnpm_global_bin:$PATH"
pnpm add --global @tech-byte-frontier/hardgate@0.4.2
printf '%s\n' "$pnpm_global_bin"
command -v hardgate
hardgate --version
```

Modern pnpm 11 uses `$PNPM_HOME/bin` as the default global executable
directory. Treat `pnpm bin --global` as the source of truth, and put its
output on `PATH` before invoking a globally installed command. If
`pnpm setup` changed your shell configuration, open a new shell before
running the remaining commands. For reproducible projects and CI, prefer an
exact local dependency with a committed lockfile. For npm,
`npm prefix --global` prints the global prefix; its `bin` directory must
also be on `PATH`.

### Native platform packages

The wrapper's current source/release contract contains six optional native
packages:

- `hardgate-linux-x64` (glibc)
- `hardgate-linux-x64-musl`
- `hardgate-linux-arm64` (glibc)
- `hardgate-linux-arm64-musl`
- `hardgate-darwin-x64`
- `hardgate-darwin-arm64`

On glibc Linux, the musl package is a fallback when the glibc package is
unavailable; a glibc binary is never selected on a musl host. The wrapper
never downloads a binary at runtime and fails closed on unsupported platforms.
Set `HARDGATE_BINARY=/absolute/path/to/hardgate` when a project supplies its
own binary. See the [wrapper README](../npm/hardgate/README.md) for resolution
details and package-manager examples.

## Shell installer and release archives

The following command is pinned to the documented released `v0.5.0` snapshot.
Verify the release page before using it:

```sh
curl -fsSL https://raw.githubusercontent.com/Tech-Byte-Frontier/hardgate/v0.5.0/scripts/install.sh | \
  HARDGATE_VERSION=v0.5.0 sh
hardgate --version
```

The installer accepts `latest`, `vX.Y.Z`, or `X.Y.Z` through
`HARDGATE_VERSION`, and `HARDGATE_INSTALL_DIR` selects the destination.
On Linux, `HARDGATE_LIBC=gnu|glibc|musl` can select the libc explicitly.
Archives contain `SHA256SUMS` and `BUILD-METADATA.json`; installation checks
the checksum, target, and exact `hardgate VERSION (COMMIT)` identity. The
supported release targets are the six Linux/macOS packages listed above.
Windows and Homebrew are not release channels in this contract.

## Upgrading

Pinned installs and lockfiles do not update automatically. To upgrade the
Cargo installation to the documented released snapshot:

```sh
cargo install hardgate --version 0.5.0 --locked --force
```

The shell installer can be upgraded to the same released snapshot:

```sh
curl -fsSL https://raw.githubusercontent.com/Tech-Byte-Frontier/hardgate/v0.5.0/scripts/install.sh | \
  HARDGATE_VERSION=v0.5.0 sh
```

The npm wrapper has no published `0.5.0` in this snapshot. Keep the exact
published `0.4.2` package until a later wrapper release is visible:

```sh
npm install --save-dev --save-exact @tech-byte-frontier/hardgate@0.4.2
pnpm add --save-dev --save-exact @tech-byte-frontier/hardgate@0.4.2
yarn up --exact @tech-byte-frontier/hardgate@0.4.2
bun add --dev --exact @tech-byte-frontier/hardgate@0.4.2
npm install --global @tech-byte-frontier/hardgate@0.4.2
pnpm add --global @tech-byte-frontier/hardgate@0.4.2
```

Review the [release notes](../CHANGELOG.md) before changing an existing policy
or Rust integration.

## Uninstalling

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

For a shell installation, first confirm which channel owns the file and then
remove the exact destination selected during installation. The default Cargo
destination is `$HOME/.cargo/bin/hardgate`:

```sh
# Only after confirming this exact file came from scripts/install.sh:
rm -- "$HOME/.cargo/bin/hardgate"
```

## Related documentation

- [Getting started](GETTING_STARTED.md) for initialization, previews, and the
  first diagnostic/refactor loop.
- [CLI reference](../docs/CLI_AND_INTEGRATION.md) for command scope, reports,
  MCP, JavaScript resolution, and native mutation.
- [Configuration specification](../docs/CONFIGURATION_SPEC.md) for presets,
  roles, budgets, evidence, and classification.
- [Release recovery](../docs/RELEASE_RECOVERY.md) for maintainer-only recovery
  and immutable artifact checks.
