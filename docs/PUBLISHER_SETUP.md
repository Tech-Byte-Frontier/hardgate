# Publisher setup

**Status:** review-only operational contract. This page describes the proposed
checked-in release workflow; it does not authorize a publication or change npm,
crates.io, GitHub Actions, repository rules, or secrets. Verify the signed
`main` workflow, the actual CI run, and public registry state before acting.

## Release identity and pins

The signed annotated `vX.Y.Z` tag identifies the release payload. The workflow
checks that the tag is an annotated object, verifies it with
`.github/release-allowed-signers`, and resolves it to the expected source
commit. `github.sha` identifies the CI-validated workflow/tooling checkout in
`release-tooling`; tooling fixes must not replace files from the signed tag.

The proposed workflow pins Node `26.8.1`, npm `12.0.2`, and these immutable action
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

The npm channels are the five native platform packages and
`@tech-byte-frontier/hardgate`:

```text
hardgate-linux-x64
hardgate-linux-arm64
hardgate-darwin-x64
hardgate-darwin-arm64
hardgate-win32-x64
@tech-byte-frontier/hardgate
```

The intended publisher sequence is all platform packages first, then the
wrapper. Each package is published at most once for the immutable version,
verified independently, and promoted to `latest` at most once after all exact
consumer evidence is merged. npm versions are immutable; an ambiguous result
requires public-state inspection and independent reconciliation within the
authorized recovery scope rather than a blind retry. Stop if identity,
integrity, or authorization remains unresolved.

## GitHub Packages npm mirror

`.github/workflows/github-packages.yml` mirrors the latest stable release of
`@tech-byte-frontier/hardgate` after the Release workflow succeeds. It can also
be dispatched manually with the latest released `vX.Y.Z` tag, including for an
existing release. The workflow must be present on `main` before it can run.

The mirror shares the release concurrency lock, verifies the signed annotated
tag and main ancestry, and requires the version to match both GitHub's latest
stable release and npm's `latest`. It copies the exact published npm tarball,
checks SHA-512 integrity and package identity, downloads the GitHub copy to
compare bytes, then installs the GitHub wrapper and checks the CLI's full
version/commit identity. Existing matching versions are reused. A conflicting
version or ambiguous publication failure stops the workflow; inspect registry
state before rerunning. The primary eight-channel release receipt stays separate
from this additional mirror workflow.

Publication uses the job's `GITHUB_TOKEN` with `packages: write`; no additional
registry secret is needed. The npm provenance flag is disabled for the GitHub
upload; the original npm tarball is preserved, but its npm registry attestation
is not copied. The package's existing repository metadata connects it to Hardgate.
GitHub initially creates packages as private: after the first successful upload,
an organization/package administrator must change package visibility to **Public**
in its GitHub package settings. Verify that setting before advertising public
availability. Organization policy may restrict package creation or visibility.

Only the scoped wrapper is mirrored. Its matching unscoped native dependency
continues to resolve from npmjs.org; GitHub's npm registry only publishes scoped
packages. Installation uses a scope-specific registry mapping, as documented in
[Installation](INSTALLATION.md#github-packages-mirror).

See GitHub's [npm registry documentation](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-npm-registry)
for authentication, repository association, and initial visibility.

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
3. All six npm package bindings, if trusted mode is selected, matching owner
   `Tech-Byte-Frontier`, repository `hardgate`, workflow filename `release.yml`,
   and any explicitly named environment.
4. The separate `NPM_TOKEN` promotion credential and its restricted use after
   exact receipt verification.
5. The signed tag, source commit, tooling commit, bundle digest, receipt
   identity, and independent public readbacks for every channel.

No local `npm whoami`, OIDC variable check, or static workflow inspection can
prove a remote trusted-publisher binding. Do not report one as verified without
the authorized publish-time evidence.

## Signed-tag signer overlap and rotation

Keep `.github/release-allowed-signers` under reviewable version control. Every
publication job must continue to require an annotated tag object (`git cat-file
-t ... = tag`), verify it with
`git -c gpg.ssh.allowedSignersFile=.github/release-allowed-signers verify-tag`,
and verify that it resolves to the expected commit. OIDC setup does not replace
this authorization.

For a planned rotation:

1. Add the new public signer through a signed, reviewed commit while retaining
   the old signer for the overlap window. Check release contract tests before
   changing the allowed-key count; do not weaken a one-signer policy implicitly.
2. Create the next signed annotated tag with the new signer and require every
   publication job to pass the existing tag and commit checks.
3. Keep the old signer until all in-flight tags and recovery work using it are
   complete. Remove it only in a later signed, reviewed commit, then rerun
   release contract checks.

If the signer is unavailable or verification fails, stop. Do not bypass the
allowlist, accept an unsigned or lightweight tag, or invent a recovery identity.

## References

- [Release recovery contract](RELEASE_RECOVERY.md)
- [Maintainer release notes](MAINTAINERS.md)
- [npm Trusted Publishers](https://docs.npmjs.com/trusted-publishers/)
- [GitHub Actions OIDC](https://docs.github.com/en/actions/reference/security/oidc)
- [crates.io Trusted Publishing](https://crates.io/docs/trusted-publishing)
- [crates.io auth action](https://github.com/rust-lang/crates-io-auth-action)
- [Cargo publish reference](https://doc.rust-lang.org/cargo/commands/cargo-publish.html)
