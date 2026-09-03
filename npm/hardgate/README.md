# hardgate (npm)

Deterministic quality gates, hard budgets, and anti-gaming verification harness for the AI agent era. Prebuilt Rust binary, no toolchain required.

```sh
npm i -D hardgate
npx hardgate check
```

Install as a devDependency, then run via `npx`, `pnpm exec`, `yarn exec`, `bunx`, or a `package.json` script. Platform binaries are delivered via `optionalDependencies` (`hardgate-linux-*`, `hardgate-darwin-*`, `hardgate-win32-*`), so installs stay offline-friendly with no postinstall downloads.

```sh
pnpm add -D hardgate
pnpm exec hardgate check --diff
```

Full docs: https://github.com/Tech-Byte-Frontier/hardgate
