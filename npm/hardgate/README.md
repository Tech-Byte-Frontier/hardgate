# @tech-byte-frontier/hardgate

[![License](https://img.shields.io/crates/l/hardgate.svg)](https://github.com/Tech-Byte-Frontier/hardgate#license)
[![GitHub Release](https://img.shields.io/github/v/release/Tech-Byte-Frontier/hardgate)](https://github.com/Tech-Byte-Frontier/hardgate/releases)

The npm wrapper launches a prebuilt Hardgate Rust binary. Install it as a development dependency and invoke it with the package manager used by the project:

```sh
npm i -D @tech-byte-frontier/hardgate
npx hardgate check

pnpm add -D @tech-byte-frontier/hardgate
pnpm exec hardgate check --diff

yarn add -D @tech-byte-frontier/hardgate
yarn exec hardgate verify

bun add -d @tech-byte-frontier/hardgate
bunx --no-install hardgate scan src/index.ts
```

## Platform packages and fallback

The v0.5.0 release contract defines exactly six Unix optional
dependencies; this matrix describes intended channel behavior and does not
claim that publication has already occurred:

- `hardgate-linux-x64` (glibc)
- `hardgate-linux-x64-musl`
- `hardgate-linux-arm64` (glibc)
- `hardgate-linux-arm64-musl`
- `hardgate-darwin-x64`
- `hardgate-darwin-arm64`

If `HARDGATE_BINARY` is set, the launcher uses that binary first. Otherwise
it resolves the package for the current OS/architecture and Linux libc. On
glibc Linux, the musl package is a fallback when the glibc optional package is
unavailable; a glibc binary is never selected on a musl host. The launcher
then permits a development Cargo binary or a real `hardgate` executable on
`PATH`. It checks candidate file types so wrapper scripts do not recurse, and
it never downloads a binary at runtime. Normal installs on unsupported
platforms fail closed.

Use `HARDGATE_BINARY=/absolute/path/to/hardgate` when the project supplies its own binary. Optional dependencies must not be omitted when the prebuilt package is expected; if they are omitted, use the explicit or Cargo/PATH fallback.

## Command scope

The wrapper forwards arguments to the Rust CLI:

```sh
npx hardgate check                 # static engines + enabled reports/freshness
npx hardgate check --diff          # changed static scope + full-index clones + diff LCOV
npx hardgate check --all           # add configured formatter/linter/test commands
npx hardgate verify                # full static + enabled evidence/ratchet
npx hardgate mutate --diff        # native baseline + AST mutants
npx hardgate init --preset strict-agent
```

No-config execution and `init --preset strict-agent` use the same preset object, including enabled coverage and mutation report policies. A generated strict template therefore needs valid report inputs. Balanced disables coverage/mutation reports; legacy-migration disables those reports and enables static reference/merge-base adoption. Missing, empty, unreadable, or malformed enabled evidence fails closed. Native `mutate` is separate from mutation-report evaluation.

Native `mutate` is Unix-only in the v0.5.0 contract and fails closed on
non-Unix builds before running commands, because robust process-group cleanup
and atomic source restoration are unavailable there. Static `check` and `scan`
remain separate commands; the six-package release matrix does not include a
Windows artifact.

For JavaScript/TypeScript mutation targets, Hardgate resolves npm, pnpm, Yarn, or Bun from the nearest package/workspace markers and infers Jest, Vitest, or Playwright from scripts, package metadata, or config files. It selects a matching test file where possible and otherwise runs the full suite. `--test-cmd` overrides resolution. See the repository [CLI reference](https://github.com/Tech-Byte-Frontier/hardgate/blob/main/docs/CLI_AND_INTEGRATION.md) for the resolution order and command forms.

## MCP

The `hardgate mcp` command is a stdio MCP server. Its static-only check tool is `hardgate_check(paths?, diff?)`; companion tools are `hardgate_scan_file(path)` and `hardgate_get_metrics(path, symbol)`. The check tool does not run reports, freshness, orchestration, dead code, or native mutation and returns explicit failures for invalid config, empty scopes, missing/unreadable files, parser/Git errors, and empty discovery.

## Release identity

The v0.5.0 shell-installer and release-archive contract supports the same six
Unix artifacts. On Linux, `HARDGATE_LIBC=gnu|glibc|musl` overrides libc
detection when needed. Archives are listed in `SHA256SUMS`;
`BUILD-METADATA.json` records target, package, version, and full source commit.
Installation verifies the unique checksum entry before extraction and
requires the binary's exact `hardgate VERSION (COMMIT)` identity. The binary
also embeds `hardgate-target:<target>`, which release verification checks.
`HARDGATE_VERSION` accepts `latest`, `vX.Y.Z`, or `X.Y.Z`;
`HARDGATE_INSTALL_DIR` chooses the destination. Windows and Homebrew are not
supported by this package contract.

Full documentation lives in the repository's [README](https://github.com/Tech-Byte-Frontier/hardgate) and [docs](https://github.com/Tech-Byte-Frontier/hardgate/tree/main/docs).
