# hardgate-darwin-arm64

hardgate prebuilt binary: macOS arm64 (Apple Silicon).

This is a platform-specific package. Install the main wrapper instead:

```sh
npm install --save-dev --save-exact @tech-byte-frontier/hardgate@0.5.0
npx hardgate check
```

The main wrapper receives this binary through `optionalDependencies`; no
postinstall download runs.

Full docs: https://github.com/Tech-Byte-Frontier/hardgate/tree/v0.5.0

License: MIT OR Apache-2.0.
