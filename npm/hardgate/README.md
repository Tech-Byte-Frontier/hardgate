# @tech-byte-frontier/hardgate (npm)

[![License](https://img.shields.io/crates/l/hardgate.svg)](https://github.com/Tech-Byte-Frontier/hardgate#license)
[![GitHub Release](https://img.shields.io/github/v/release/Tech-Byte-Frontier/hardgate)](https://github.com/Tech-Byte-Frontier/hardgate/releases)

Deterministic quality gates, hard budgets, and anti-gaming verification harness for the AI agent era. Prebuilt Rust binary, no toolchain required.

```sh
npm i -D @tech-byte-frontier/hardgate
npx hardgate check
```

Install as a devDependency, then run via `npx`, `pnpm exec`, `yarn dlx`, `bunx`, or a `package.json` script. Platform binaries are delivered via `optionalDependencies` (`hardgate-linux-*`, `hardgate-darwin-*`, `hardgate-win32-*`), so installs stay offline-friendly with no postinstall downloads.

```sh
pnpm add -D @tech-byte-frontier/hardgate
pnpm exec hardgate check --diff

# yarn / bun
yarn add -D @tech-byte-frontier/hardgate
yarn dlx hardgate check
bun add -d @tech-byte-frontier/hardgate
bunx hardgate check
```

## Common commands

```sh
npx hardgate check                 # full gate (<200ms)
npx hardgate check --diff          # git-modified files only (sub-10ms, pre-commit)
npx hardgate check --all --dead-code
npx hardgate scan src/index.ts     # 1ms AST metric inspection
npx hardgate init --preset strict-agent
```

Set `HARDGATE_BINARY` to override the resolved binary.

## Docs

Full docs: https://github.com/Tech-Byte-Frontier/hardgate

* Configuration (`hardgate.toml`), CLI reference, MCP / agent integration, and architecture live in [`docs/`](https://github.com/Tech-Byte-Frontier/hardgate/tree/main/docs).

## License

Dual-licensed under either MIT or Apache-2.0, at your option. See `LICENSE-MIT` and `LICENSE-APACHE`.
