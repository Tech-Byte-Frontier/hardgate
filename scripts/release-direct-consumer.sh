#!/usr/bin/env bash
# Download and check the actual public GNU archive from the signed release.
set -euo pipefail
case "${1:-}" in
  exact) state=exact_consumer_verified ;;
  default)
    state=default_consumer_verified
    test "$(gh release view --json tagName --jq .tagName)" = "$RELEASE_TAG"
    gh release view "$RELEASE_TAG" --json isDraft,isPrerelease --jq '(.isDraft == false) and (.isPrerelease == false)' | grep -qx true
    ;;
  *) echo 'usage: scripts/release-direct-consumer.sh exact|default' >&2; exit 2 ;;
esac
install_root=$(mktemp -d)
trap 'rm -rf -- "$install_root"' EXIT
# The shared checksum manifest covers every platform, so download and compare
# the entire verified bundle before checking it and running the host binary.
for expected_asset in dist/*; do
  asset=${expected_asset##*/}
  gh release download "$RELEASE_TAG" --pattern "$asset" --dir "$install_root"
  cmp -- "dist/$asset" "$install_root/$asset"
done
(cd "$install_root" && sha256sum --check --strict SHA256SUMS)
tar -xzf "$install_root/hardgate-linux-x64.tar.gz" -C "$install_root"
binary="$install_root/hardgate-linux-x64/hardgate"
test "$("$binary" --version)" = "hardgate ${RELEASE_VERSION} (${RELEASE_COMMIT})"
node release-tooling/scripts/installed-check.mjs "$binary"
node release-tooling/scripts/release-receipt-cli.mjs advance \
  --receipt receipt/release.json --channel github-assets --to "$state" --consumer "$binary"
