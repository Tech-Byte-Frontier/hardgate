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

The npm wrapper requires Node.js 18 or newer.

## Platform packages and fallback

The v0.5.0 release contract defines exactly six Linux/macOS optional
dependencies (Linux x64/arm64 glibc and musl, macOS x64/arm64); this matrix
describes intended channel behavior and does not claim that publication has
already occurred:

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
npx hardgate check --diff          # Git-changed/staged + explicit paths; diff LCOV
npx hardgate check --all           # add configured formatter/linter/test commands
npx hardgate verify                # full static + enabled evidence/ratchet
npx hardgate mutate --diff        # native baseline + AST mutants when enabled
npx hardgate init --preset strict-agent
```

`verify` path arguments only narrow current static/dead-code inventory and
coverage source matching; mutation reports and generated freshness remain
configured/full checks. The ratchet still loads and validates the full
configured reference snapshot, then compares it only to selected current
static/dead-code findings without widening explicit paths.

No-config execution and `init --preset strict-agent` use the same preset object, including enabled coverage and mutation report policies. A generated strict template therefore needs valid report inputs. Balanced disables coverage/mutation reports; legacy-migration disables those reports and enables static reference/merge-base adoption. Missing, empty, unreadable, or malformed enabled evidence fails closed. Native `mutate` is separate from mutation-report evaluation. If `[mutation].enabled = false`, it prints a disabled-policy note and succeeds without target discovery, baseline execution, or mutants; target/no-target rules apply only when enabled.

Native `mutate` is available to source builds on Linux and macOS through the
target-OS cfg; on other operating systems it fails closed before baseline or
source writes because robust process-group cleanup and atomic source
restoration are unavailable there. Static `check` and `scan` remain separate
commands. The prebuilt, npm, and shell-installer release contract remains
exactly six x64/arm64 glibc/musl/macOS targets (Linux x64/arm64 glibc and musl,
macOS x64/arm64), which does not constrain source builds.

After an explicit scope is validated, a `mutate --diff` run (including
`--scoped`) with no changed production source is a successful no-op. Missing,
invalid, unsupported, or non-source explicit scopes fail closed; only a non-diff
unrestricted or scoped run with no eligible source target fails.

For JavaScript/TypeScript mutation targets, Hardgate validates encountered
package manifests, recognizes only declared workspaces (lockfiles are manager
hints), and resolves npm, pnpm, Yarn, or Bun. A child `test` script wins; one
unambiguous `test:*` script is allowed, and a reliable child-local manifest,
framework-config, or script signal wins over a validated enclosing
workspace-root script. Framework selection uses only those manifest, config,
and unambiguous script signals; it does not scan dependency packages.
That root script is used only with no local script or reliable local manifest/config/script signal;
malformed manifests or ambiguous scripts fail closed. It infers
Jest, Vitest, or Playwright only when selector behavior is unambiguous, selects
a matching test where possible, and otherwise runs the full suite.
`--test-cmd` overrides resolution. See the repository [CLI reference](https://github.com/Tech-Byte-Frontier/hardgate/blob/main/docs/CLI_AND_INTEGRATION.md) for the resolution order and command forms.

## MCP

The `hardgate mcp` command is a stdio MCP server. Its static-only check tool is `hardgate_check(paths?, diff?)`; companion tools are `hardgate_scan_file(path)` and `hardgate_get_metrics(path, symbol)`. `diff` selects Git-changed/staged inventory by default, while explicit existing paths add to static/clone selection and clone matching uses the full repository index. MCP never runs coverage or other reports, freshness, orchestration, dead code, or native mutation. Invalid arguments/configuration, missing paths, empty scopes/discovery, and Git failures are outer tool errors. Read/parse failures remain report-level Hardgate `Failed` findings whose effective role severity makes them errors, advisories, or omitted findings (`error`, `warning`, or `ignore`). For `hardgate_scan_file`, a read failure is an outer tool error while parse/static findings remain in its per-file report; `hardgate_get_metrics` reports read or missing-symbol errors explicitly.

## Release identity

The v0.5.0 shell-installer and release-archive contract supports exactly the
six Linux/macOS artifacts listed above (Linux x64/arm64 glibc and musl, macOS
x64/arm64). On Linux, `HARDGATE_LIBC=gnu|glibc|musl` explicitly selects
the libc and takes precedence over automatic detection. Archives are listed in `SHA256SUMS`;
`BUILD-METADATA.json` records target, package, version, and full source commit.
Installation verifies the unique checksum entry before extraction and
requires the binary's exact `hardgate VERSION (COMMIT)` identity. The binary
also embeds `hardgate-target:<target>`, which release verification checks.
`HARDGATE_VERSION` accepts `latest`, `vX.Y.Z`, or `X.Y.Z`;
`HARDGATE_INSTALL_DIR` chooses the destination. Windows and Homebrew are not
supported by this package contract.

## Upgrading from 0.4.2

Pinned installs and lockfiles do not auto-update. Upgrade explicitly to 0.5.0:

```sh
cargo install hardgate --version 0.5.0 --locked --force
npm install --save-dev @tech-byte-frontier/hardgate@0.5.0
pnpm add --save-dev @tech-byte-frontier/hardgate@0.5.0
yarn up @tech-byte-frontier/hardgate@0.5.0
bun add --dev @tech-byte-frontier/hardgate@0.5.0
```

Review the [v0.5.0 migration notes](https://github.com/Tech-Byte-Frontier/hardgate/blob/v0.5.0/CHANGELOG.md#050) for policy and Rust API changes.

Full documentation lives in the repository's [README](https://github.com/Tech-Byte-Frontier/hardgate/tree/v0.5.0) and [docs](https://github.com/Tech-Byte-Frontier/hardgate/tree/v0.5.0/docs).
