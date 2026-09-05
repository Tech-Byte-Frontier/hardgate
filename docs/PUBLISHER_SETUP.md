# Publisher setup

**Status:** review-only operational contract. This page does not authorize a
publication or change npm, crates.io, GitHub Actions, repository rules, or
secrets. It was checked on 2026-09-04 against the local dirty
`/tmp/hardgate-audit-20260904/release-flow` proposal. That worktree and its
helper branches are not deployed evidence; confirm the signed `main` workflow,
the actual CI run, and public registry state before acting.

## Release identity and pins

The signed annotated `vX.Y.Z` tag identifies the release payload. The workflow
checks that the tag is an annotated object, verifies it with
`.github/release-allowed-signers`, and resolves it to the expected source
commit. `github.sha` identifies the CI-validated workflow/tooling checkout in
`release-tooling`; tooling fixes must not replace files from the signed tag.

The local proposal pins Node `26.8.1`, npm `12.0.2`, and these immutable action
commits:

| Action | Commit | Release label |
| --- | --- | --- |
| `actions/checkout` | `3d3c42e5aac5ba805825da76410c181273ba90b1` | `v7.0.1` |
| `actions/setup-node` | `820762786026740c76f36085b0efc47a31fe5020` | `v7.0.0` |
| `actions/download-artifact` | `3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c` | `v8.0.1` |
| `actions/upload-artifact` | `043fb46d1a93c77aae656e7c1c64a875d1fc6a0a` | `v7.0.1` |

Re-read the live workflow before relying on these values. A local checkout is
not proof that the remote workflow or publisher configuration has changed.

## npm authentication modes

`NPM_PUBLISH_AUTH_MODE` selects exactly one npm publication mode. An unset
repository variable defaults to `token`; any supplied value must be `token` or
`trusted`. There is no implicit fallback between modes.

- **`token`:** publication receives the existing `secrets.NPM_TOKEN` as
  `NODE_AUTH_TOKEN`. The preflight `npm whoami` check is evidence for token
  authentication only; it does not prove an OIDC trusted-publisher binding.
- **`trusted`:** publication receives GitHub's OIDC request credentials and
  strips registry tokens from the publish environment. The job needs
  `id-token: write`; a failed OIDC exchange stops the run and does not fall back
  to `NPM_TOKEN`. Presence of OIDC variables is a prerequisite, not proof that
  npm has the package binding; npm publish is the authoritative binding check.

The pinned npm `12.0.2` publisher does not use OIDC to perform a dist-tag
promotion. Therefore every promotion from `hardgate-candidate` to `latest`
must use the existing `secrets.NPM_TOKEN` separately, including when package
publication used `trusted` mode. This is not a new secret and must not be
silently substituted or omitted. Keep the promotion credential scoped to the
promotion operation, and never print it or carry it into read-only verifiers.

The seven npm channels are the six platform packages and
`@tech-byte-frontier/hardgate`:

```text
hardgate-linux-x64
hardgate-linux-x64-musl
hardgate-linux-arm64
hardgate-linux-arm64-musl
hardgate-darwin-x64
hardgate-darwin-arm64
@tech-byte-frontier/hardgate
```

The intended publisher sequence is all six platform packages first, then the
wrapper. Each package is published at most once for the immutable version,
verified independently, and promoted to `latest` at most once after all exact
consumer evidence is merged. npm versions are immutable; an ambiguous result
requires public-state inspection and maintainer review rather than a blind
retry.

## crates.io authentication

The existing Cargo flow remains token-based through
`secrets.CARGO_REGISTRY_TOKEN`. Preserve its exact-version probe, publication,
and independent install/identity verification. Do not claim that npm or GitHub
success covers crate publication; the release receipt keeps the crate channel
separate and the final check verifies the intended `max_stable_version`.

A crates.io trusted-publisher setup is deferred. It requires separately
authorized external configuration and a reviewed pinned auth action; this page
does not claim that configuration exists and does not add a fallback mode.

## Activation evidence

Before enabling or changing a mode, an authorized maintainer must record:

1. The signed reviewed workflow commit and the exact workflow filename
   `release.yml`.
2. The selected `token` or `trusted` mode, job-scoped permissions, and the
   absence of registry tokens in trusted publish environments.
3. The seven npm package bindings, if trusted mode is selected, matching owner
   `Tech-Byte-Frontier`, repository `hardgate`, workflow filename `release.yml`,
   and any explicitly named environment.
4. The separate `NPM_TOKEN` promotion credential and its restricted use after
   exact receipt verification.
5. The signed tag, source commit, tooling commit, bundle digest, receipt
   identity, and independent public readbacks for every channel.

No local `npm whoami`, OIDC variable check, or static workflow inspection can
prove a remote trusted-publisher binding. Do not report one as verified without
the authorized publish-time evidence.

## References

- [Release recovery contract](RELEASE_RECOVERY.md)
- [Maintainer release notes](MAINTAINERS.md)
- [npm Trusted Publishers](https://docs.npmjs.com/trusted-publishers/)
- [GitHub Actions OIDC](https://docs.github.com/en/actions/reference/security/oidc)
- [crates.io Trusted Publishing](https://crates.io/docs/trusted-publishing)
- [crates.io auth action](https://github.com/rust-lang/crates-io-auth-action)
- [Cargo publish reference](https://doc.rust-lang.org/cargo/commands/cargo-publish.html)
