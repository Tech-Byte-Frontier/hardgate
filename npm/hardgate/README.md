# @tech-byte-frontier/hardgate

A thin npm launcher for the Hardgate Rust CLI. Hardgate checks Rust and
JavaScript/TypeScript structural policy, formatting, linting, configured tests,
type checks, and required source-bound coverage or mutation evidence.

## Install

Use npm or pnpm with optional dependencies enabled:

```sh
npm install --save-dev --save-exact @tech-byte-frontier/hardgate
npx --no-install hardgate init
npx --no-install hardgate check --checks policy

pnpm add --save-dev --save-exact @tech-byte-frontier/hardgate
pnpm exec hardgate check --checks policy
```

This source README describes 0.6; a source manifest version does not establish
registry availability. Select a published version and commit your lockfile.

## Supported runtime

Local analysis and ordinary project checks support Linux (x64/ARM64 glibc 2.39+) and macOS
(Intel/Apple Silicon). Node.js 18+ runs the launcher and the
matching native optional dependency supplies the executable. **Rust is not
required.**

`scan`, policy-only checks, saved reports, and static MCP tools run without
platform isolation. Policy-only checks are partial and still require configured
saved evidence. Ordinary `check` and `fmt` run natively. Evidence producers and
`orchestration.require_isolation = true` require Linux cgroup v2 and systemd
254+ (or inherited verified limits), plus Landlock ABI 3+ for protected checks.
Unsupported execution features fail with setup guidance before starting tools.

npm and pnpm are tested installation channels. Windows and musl/Alpine are not supported. Published versions retain their original
platform contracts.

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
