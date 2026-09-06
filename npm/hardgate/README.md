# @tech-byte-frontier/hardgate

A thin npm launcher for the Hardgate Rust CLI. Hardgate checks Rust and
JavaScript/TypeScript structural policy, formatting, linting, configured tests,
type checks, and required source-bound coverage or mutation evidence.

## Install

Use npm or pnpm with optional dependencies enabled:

```sh
npm install --save-dev --save-exact @tech-byte-frontier/hardgate
npx --no-install hardgate init
npx --no-install hardgate check

pnpm add --save-dev --save-exact @tech-byte-frontier/hardgate
pnpm exec hardgate check
```

This source README describes 0.6; a source manifest version does not establish
registry availability. Select a published version and commit your lockfile.

## Supported runtime

Linux x64 GNU only. The prebuilt baseline is Ubuntu 24.04 with glibc 2.39+.
Node.js 18+ runs the launcher. Workload commands require cgroup-v2 CPU/memory/swap
and task limits, systemd 254+ with an accessible user manager (or inherited
verified limits), and enabled Landlock ABI 3+ for read-only child checks.
Install the project's configured tools separately. `--version` does not prove
that the host can run `check`.

ARM64, musl/Alpine, macOS, and Windows are unsupported in 0.6. npm and pnpm are
the tested package managers. Previously published artifacts retain their own
release contracts.

The matching `hardgate-linux-x64` optional dependency supplies the binary.
There are no postinstall or runtime downloads. The launcher first resolves the
installed native package, then a local Rust workspace binary or a real binary
on PATH. It rejects launcher scripts to prevent recursion.
`HARDGATE_BINARY=/absolute/path/to/hardgate` explicitly overrides resolution on
a supported host. Exit status, arguments, and signals pass through to the CLI.

For global use, install with npm or pnpm's `--global` option and verify
`command -v hardgate`; `pnpm bin --global` identifies pnpm's executable directory.
Cargo and direct release downloads are also supported. There is no separate
shell installer in 0.6.

[Installation and runtime setup](https://github.com/Tech-Byte-Frontier/hardgate/blob/main/docs/INSTALLATION.md)
· [CLI reference](https://github.com/Tech-Byte-Frontier/hardgate/blob/main/docs/CLI_AND_INTEGRATION.md)
· [Releases](https://github.com/Tech-Byte-Frontier/hardgate/releases)

License: MIT OR Apache-2.0.
