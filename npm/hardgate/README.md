# @tech-byte-frontier/hardgate

[![License](https://img.shields.io/crates/l/hardgate.svg)](https://github.com/Tech-Byte-Frontier/hardgate#license)
[![GitHub Release](https://img.shields.io/github/v/release/Tech-Byte-Frontier/hardgate)](https://github.com/Tech-Byte-Frontier/hardgate/releases)

The npm wrapper launches a prebuilt Hardgate Rust binary. Install it as a development dependency and run the CLI with your package manager:

```sh
npm i -D @tech-byte-frontier/hardgate
npx hardgate check

pnpm add -D @tech-byte-frontier/hardgate
pnpm exec hardgate check --diff

yarn add -D @tech-byte-frontier/hardgate
yarn dlx hardgate verify

bun add -d @tech-byte-frontier/hardgate
bunx hardgate scan src/index.ts
```

The wrapper's optional dependencies are the six published Unix platform packages:

- hardgate-linux-x64 (glibc)
- hardgate-linux-x64-musl
- hardgate-linux-arm64 (glibc)
- hardgate-linux-arm64-musl
- hardgate-darwin-x64
- hardgate-darwin-arm64

The launcher selects the package for the current platform and libc, checks discovered package candidates are machine binaries, and falls back between Linux libc variants or a development/Cargo/PATH binary when available. It does not fetch a binary at runtime. Set HARDGATE_BINARY to use an explicit binary path.

## Command scope

The wrapper forwards arguments to the Rust CLI. In particular:

```sh
npx hardgate check                 # static engines plus enabled report checks
npx hardgate check --all           # add configured formatter/linter/test commands
npx hardgate verify                # evaluate enabled LCOV/mutation reports
npx hardgate mutate --diff         # execute the native AST mutation loop
npx hardgate init --preset strict-agent
```

Coverage and mutation report checks are disabled in the initialized template until a project supplies report paths. A strict enabled policy fails when required evidence is missing or malformed; a disabled policy does not consume stale files. Native mutate is separate from Stryker report evaluation.

Full documentation lives in the repository's [README](https://github.com/Tech-Byte-Frontier/hardgate) and [docs](https://github.com/Tech-Byte-Frontier/hardgate/tree/main/docs).
