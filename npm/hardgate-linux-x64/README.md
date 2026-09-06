# hardgate-linux-x64

Hardgate's Linux x64 GNU prebuilt binary. Install the main wrapper with npm or
pnpm; it selects this package through an exact optional dependency:

```sh
npm install --save-dev --save-exact @tech-byte-frontier/hardgate
npx --no-install hardgate check
```

The 0.6 baseline is Ubuntu 24.04 x64 with glibc 2.39+, cgroup v2 resource
controllers, systemd 254+ with a user manager (or inherited verified limits),
and Landlock ABI 3+ enabled. `--version` alone does not prove check support.
No postinstall download runs. ARM64, musl, and macOS are deferred.

[Runtime and installation guide](https://github.com/Tech-Byte-Frontier/hardgate/blob/main/docs/INSTALLATION.md)
· [Releases](https://github.com/Tech-Byte-Frontier/hardgate/releases)

License: MIT OR Apache-2.0.
