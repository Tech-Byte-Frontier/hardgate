# hardgate-darwin-x64

Native Hardgate binary for `x86_64-apple-darwin`. Installed automatically by
`@tech-byte-frontier/hardgate`; no Rust toolchain is needed.

Ordinary checks and formatting run natively. Evidence producers and policies
with `orchestration.require_isolation = true` require Linux resource containment
and Landlock for protected child checks.
