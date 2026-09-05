# Publisher setup proposal

**Status:** review-only; this does not authorize a publish or change npm, crates.io,
GitHub Actions, repository rules, or secrets. **Verified:** 2026-09-04 against this
checkout and the official sources linked below.

## Current release state

The active workflow is `.github/workflows/release.yml`. Repository-wide pins are Node
`26.8.1`, npm `12.0.2` (the workflow `env` block),
`actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1` (`v7.0.1`), and
`actions/setup-node@820762786026740c76f36085b0efc47a31fe5020` (`v7.0.0`).

`publish-npm` runs on GitHub-hosted `ubuntu-24.04` with job-scoped `contents: read`,
`actions: read`, and `id-token: write`. `publish-crates` has no job-level permissions
block and inherits `contents: read` and `actions: read` from the workflow defaults; it
cannot request OIDC today.
The active authentication mode is **token**:

- `publication-preflight` requires `secrets.NPM_TOKEN` as `NODE_AUTH_TOKEN`
  in the `Authenticate the npm publication credential` step; `publish-npm` uses it in
  `Publish and verify each platform package in order` and `Publish wrapper only after all platforms are verified`.
- `publication-preflight` and `publish-crates` require `secrets.CARGO_REGISTRY_TOKEN`
  in `Require the crates.io publication credential` and `Publish crate when exact version is missing`, respectively.
- The npm child process runs `npm publish --provenance --access public --ignore-scripts`;
  the token is unset before the publication verifier.
No remote trusted-publisher setting, GitHub environment, or secret value was read. Treat
each external binding below as unconfigured until a maintainer records fresh UI/API read
evidence during an authorized setup change.
## Proposed authentication modes

The workflow should select exactly one explicit mode per registry: `token` or `trusted`.
An absent, malformed, or mixed mode must fail before publication. A failed OIDC exchange
must never silently fall back to a long-lived token in the same run.
### npm: current token mode

Keep the current `NPM_TOKEN` secret and `NODE_AUTH_TOKEN` handoff while trusted publishing
is reviewed. Keep the non-empty check and `npm whoami` token preflight; do not print the
value or leave it set for the verifier.
### npm: proposed trusted-publisher mode

Configure these seven packages independently in npm package settings:

`hardgate-linux-x64`, `hardgate-linux-x64-musl`, `hardgate-linux-arm64`,
`hardgate-linux-arm64-musl`, `hardgate-darwin-x64`, `hardgate-darwin-arm64`, and
`@tech-byte-frontier/hardgate`.
For every package, propose this binding:

| Field | Value |
| --- | --- |
| Provider | GitHub Actions |
| Owner | `Tech-Byte-Frontier` |
| Repository | `hardgate` |
| Workflow filename | `release.yml` |
| Environment | unset; add only if the workflow later names an exact environment |

npm asks for the workflow **filename**, not `.github/workflows/release.yml`. Every current
manifest points `repository.url` to `git+https://github.com/Tech-Byte-Frontier/hardgate.git`;
recheck all seven before saving the bindings.
npm Trusted Publishing requires a GitHub-hosted runner, npm CLI `>=11.5.1`, Node
`>=22.14.0`, `contents: read`, and `id-token: write`. The current publisher pins Node
`26.8.1` and npm `12.0.2`, so this publisher runtime meets those minimums without raising
the package consumer engine floor of Node `>=18`.

Retain setup-node's registry URL, the provenance publish command, and the exact action
SHAs above. Trusted mode must remove `NODE_AUTH_TOKEN` from the publish environment.
`npm whoami` is a token check, not an OIDC configuration test; npm says a trusted-publisher
mismatch is detected at publish time. Static checks must fail closed and the package loop
must stop on the first trusted-auth failure.
### crates.io: current token mode

Keep `CARGO_REGISTRY_TOKEN` in the current mode. The workflow makes an anonymous exact
version-state probe and publishes only when that version is absent; it has no non-mutating
registry credential check. Preserve this behavior and never log the token.
### crates.io: proposed trusted-publisher mode

Configure the `hardgate` crate once in the crates.io Trusted Publishing UI:

```text
Provider: GitHub Actions
Owner: Tech-Byte-Frontier
Repository: hardgate
Workflow filename: release.yml
Environment: unset unless the workflow is explicitly changed to use one
```
Crates.io requires an initial manual publication before creating a trusted-publisher
configuration. Confirm that prerequisite in the crates.io UI; the workflow version probe
does not prove the external setting exists.
Trusted mode requires job-scoped `id-token: write` and `contents: read`, plus the official
`rust-lang/crates-io-auth-action` pinned to a reviewed full commit SHA. The official v1.0.0
release identifies `63a7064947ceca9989005e118db3a5fecdc9259f`; reverify before use. Pass only
that action's temporary-token output to `cargo publish`, then unset it. The current workflow
has neither the permission nor the action; do not copy the action docs' floating `@v1` into this
pinned workflow. The action's post-job revocation behavior should remain enabled.
The current documented crates.io flow is GitHub Actions: it exchanges GitHub OIDC identity
for a short-lived registry token; it does not make `cargo publish` transactional. An exchange
failure is terminal for the job, with no automatic `CARGO_REGISTRY_TOKEN` fallback.
## Fail-closed activation checks

Before selecting trusted mode, an authorized workflow change must verify:

1. The job is GitHub-hosted and the exact workflow filename is `release.yml`.
2. Each trusted publisher job keeps job-scoped `contents: read` and `actions: read` (needed by artifact downloads) and adds `id-token: write`; do not rely on broader workflow defaults.
3. Node/npm meet npm's documented minimums and package metadata identifies
   `Tech-Byte-Frontier/hardgate`.
4. Each external binding matches owner, repository, workflow file, and any environment
   claim exactly; an unknown environment is an error.
5. Trusted mode has no registry token in its environment. Token mode has its named secret
   and no OIDC exchange step.
6. The signed annotated-tag check succeeds before authentication and immediately before
   publication, and the tag resolves to the expected commit.

Local preflight may validate declarations and permissions, but must not pretend to prove a
remote trusted-publisher match without exchanging a token. The first authorized trusted
publish is the definitive external check. Stop before the next package or registry on
failure, retain receipts for successful operations, and run existing public-state verifiers.
The npm packages, crate, and GitHub release are separate systems; this proposal makes no
atomic cross-registry transaction claim.

Keep token mode available until an authorized run verifies every trusted binding. Retire
old secrets only in a separate reviewed change after that evidence. A failed trusted run
may be retried in explicit token mode only after a maintainer changes and reviews the mode.
## Signed-tag signer overlap and rotation

Keep `.github/release-allowed-signers` under reviewable version control. Every publication
job must continue to require an annotated tag object (`git cat-file -t ... = tag`), verify
it with `git -c gpg.ssh.allowedSignersFile=.github/release-allowed-signers verify-tag`, and
verify that it resolves to the expected commit. OIDC setup does not replace this authorization.

For a planned rotation:

1. Add the new public signer through a signed, reviewed commit while retaining the old signer
   for the overlap window. Check release contract tests before changing the allowed-key count;
   do not weaken a one-signer policy implicitly.
2. Create the next signed annotated tag with the new signer and require every publication job
   to pass the existing tag and commit checks.
3. Keep the old signer until all in-flight tags and recovery work using it are complete. Remove
   it only in a later signed, reviewed commit, then rerun release contract checks.

If the signer is unavailable or verification fails, stop. Do not bypass the allowlist, accept
an unsigned/lightweight tag, or invent a recovery identity.

## Official sources

- [npm Trusted Publishers](https://docs.npmjs.com/trusted-publishers/) — GitHub fields,
  GitHub-hosted limitation, Node/npm minimums, permissions, provenance, and migration.
- [GitHub Actions OIDC reference](https://docs.github.com/en/actions/reference/security/oidc)
  — `id-token: write` and workflow identity claims.
- [crates.io Trusted Publishing](https://crates.io/docs/trusted-publishing) — current
  registry setup and supported claims; re-read its UI instructions at activation.
- [Rust crates.io development update](https://blog.rust-lang.org/2025/07/11/crates-io-development-update-2025-07/)
  — GitHub Actions OIDC, initial manual publication, and the auth-action flow.
- [rust-lang/crates-io-auth-action](https://github.com/rust-lang/crates-io-auth-action) and
  its [official releases](https://github.com/rust-lang/crates-io-auth-action/releases) —
  temporary-token output, post-job revocation, and reviewed action pins.
- [Cargo publish reference](https://doc.rust-lang.org/cargo/commands/cargo-publish.html) —
  publish and credential behavior.

Re-verify links, external bindings, action SHAs, and permissions immediately before enabling
trusted mode. This proposal records no authorization to do so.
