# hardgate-linux-x64

Hardgate's Linux x64 GNU prebuilt binary. Install the main wrapper with npm or
pnpm; it selects this package through an exact optional dependency:

```sh
npm install --save-dev --save-exact @tech-byte-frontier/hardgate
npx --no-install hardgate check --checks policy
```

The prebuilt baseline is Ubuntu 24.04 x64 with glibc 2.39+. Static analysis
needs no cgroup or systemd setup. Executing project tools requires cgroup v2,
systemd 254+ (or inherited verified limits), and Landlock ABI 3+ for read-only
child checks. No Rust or postinstall download is needed for this package.
Other native packages cover Linux ARM64, macOS, and Windows x64.

[Runtime and installation guide](https://github.com/Tech-Byte-Frontier/hardgate/blob/main/docs/INSTALLATION.md)
· [Releases](https://github.com/Tech-Byte-Frontier/hardgate/releases)

License: MIT OR Apache-2.0.
