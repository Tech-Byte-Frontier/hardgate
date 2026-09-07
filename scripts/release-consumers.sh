#!/usr/bin/env bash
# Verify real project/global npm and pnpm installs against the signed payload.
# Called from its payload checkout with CI-validated tooling in release-tooling/.
set -euo pipefail
case "${1:-}" in
  exact) selector="${RELEASE_VERSION:?RELEASE_VERSION is required}" ;;
  default) selector=latest ;;
  *) echo 'usage: scripts/release-consumers.sh exact|default' >&2; exit 2 ;;
esac
set -euo pipefail
unset HARDGATE_BINARY HARDGATE_BINARY_PATH HARDGATE_NATIVE_BINARY HARDGATE_LAUNCHER_DEPTH NODE_OPTIONS HARDGATE_VERSION
consumer_tmp=$(mktemp -d)
trap 'rm -rf "$consumer_tmp"' EXIT
trap 'exit 130' HUP INT TERM
private_bin="$consumer_tmp/private-bin"
mkdir -p "$private_bin"
printf '%s\n' '#!/bin/sh' 'exit 127' > "$private_bin/hardgate"
chmod 755 "$private_bin/hardgate"
node_tool=$(command -v node)
npm_tool=$(command -v npm)
pnpm_tool=$(command -v pnpm)
node_bin=$(dirname "$node_tool")
manager_path="$private_bin:$node_bin:$(dirname "$npm_tool"):$(dirname "$pnpm_tool"):/usr/bin:/bin"
runtime_path="$private_bin:$node_bin:/usr/bin:/bin"
expected="hardgate $RELEASE_VERSION ($RELEASE_COMMIT)"
expected_native="$consumer_tmp/hardgate-linux-x64"
tar xOf "dist/hardgate-linux-x64.tar.gz" hardgate-linux-x64/hardgate > "$expected_native"
chmod 755 "$expected_native"
wrapper_source="$PWD/npm/hardgate/bin/hardgate.js"
verify_script="$PWD/release-tooling/scripts/verify-installed-wrapper.mjs"
validation_path="$runtime_path:$(dirname "$(command -v cargo)")"
validation_rustup="${RUSTUP_HOME:-$HOME/.rustup}"
acceptance_script="$PWD/release-tooling/scripts/installed-check.mjs"
make_project() {
  project="$1"
  mkdir -p "$project"
  printf '%s\n' '{"name":"hardgate-default-consumer","private":true,"version":"1.0.0"}' > "$project/package.json"
}
verify_install() {
  root="$1"; command="$2"; home="$3"; label="$4"
  version_output=$(env -i HOME="$home" PATH="$runtime_path" "$command" --version)
  test "$version_output" = "$expected"
  env -i HOME="$home" PATH="$runtime_path" "$node_tool" "$verify_script" \
    "$root" "$command" "$expected_native" "$wrapper_source" "$RELEASE_VERSION" "$label"
  env -i HOME="$home" PATH="$validation_path" RUSTUP_HOME="$validation_rustup" RUSTUP_TOOLCHAIN="$RUST_TOOLCHAIN" \
    "$node_tool" "$acceptance_script" "$command"
}

npm_project="$consumer_tmp/npm-project"; npm_home="$consumer_tmp/npm-home"
npm_cache="$consumer_tmp/npm-cache"; npm_config="$consumer_tmp/npmrc"
make_project "$npm_project"; mkdir -p "$npm_home" "$npm_cache"
printf '%s\n' 'audit=false' 'fund=false' 'ignore-scripts=true' 'include=optional' > "$npm_config"
env -i HOME="$npm_home" PATH="$manager_path" NPM_CONFIG_USERCONFIG="$npm_config" \
  NPM_CONFIG_CACHE="$npm_cache" NPM_CONFIG_REGISTRY=https://registry.npmjs.org/ \
  NPM_CONFIG_AUDIT=false NPM_CONFIG_FUND=false NPM_CONFIG_FETCH_RETRIES=0 \
  NPM_CONFIG_FETCH_TIMEOUT=20000 NPM_CONFIG_IGNORE_SCRIPTS=true "$npm_tool" install --ignore-scripts --prefix "$npm_project" \
  --include=optional "@tech-byte-frontier/hardgate@$selector"
verify_install "$npm_project" "$npm_project/node_modules/.bin/hardgate" "$npm_home" npm-project

pnpm_project="$consumer_tmp/pnpm-project"; pnpm_home="$consumer_tmp/pnpm-home"
pnpm_config="$consumer_tmp/pnpm-config"; pnpm_store="$consumer_tmp/pnpm-store"
# These disposable consumers verify the just-published, signed release bytes.
# pnpm 11 otherwise selects an older version during its default one-day delay.
make_project "$pnpm_project"; mkdir -p "$pnpm_home" "$pnpm_config" "$pnpm_store"
(cd "$pnpm_project" && env -i HOME="$pnpm_home" XDG_CONFIG_HOME="$pnpm_config" \
  pnpm_config_minimum_release_age=0 PNPM_STORE_DIR="$pnpm_store" PATH="$manager_path" "$pnpm_tool" add \
  --ignore-scripts --registry=https://registry.npmjs.org/ --store-dir "$pnpm_store" "@tech-byte-frontier/hardgate@$selector")
verify_install "$pnpm_project" "$pnpm_project/node_modules/.bin/hardgate" "$pnpm_home" pnpm-project

npm_global="$consumer_tmp/npm-global"; npm_global_home="$consumer_tmp/npm-global-home"
npm_global_cache="$consumer_tmp/npm-global-cache"; npm_global_config="$consumer_tmp/npm-global-npmrc"
mkdir -p "$npm_global" "$npm_global_home" "$npm_global_cache"
printf '%s\n' 'audit=false' 'fund=false' 'ignore-scripts=true' > "$npm_global_config"
env -i HOME="$npm_global_home" PATH="$manager_path" NPM_CONFIG_USERCONFIG="$npm_global_config" \
  NPM_CONFIG_CACHE="$npm_global_cache" NPM_CONFIG_REGISTRY=https://registry.npmjs.org/ \
  NPM_CONFIG_AUDIT=false NPM_CONFIG_FUND=false NPM_CONFIG_IGNORE_SCRIPTS=true "$npm_tool" install --ignore-scripts --global --prefix "$npm_global" \
  "@tech-byte-frontier/hardgate@$selector"
npm_global_command="$npm_global/bin/hardgate"; test -x "$npm_global_command"
env -i HOME="$npm_global_home" EXPECTED="$expected" PATH="$npm_global/bin:$private_bin:$node_bin:/usr/bin:/bin" \
  sh -c 'test "$(command -v hardgate)" = "$1" && test "$(hardgate --version)" = "$EXPECTED"' sh "$npm_global_command"
verify_install "$npm_global" "$npm_global_command" "$npm_global_home" npm-global

pnpm_global="$consumer_tmp/pnpm-global"; pnpm_global_bin_expected="$pnpm_global/bin"; pnpm_global_home="$consumer_tmp/pnpm-global-home"
pnpm_global_config="$consumer_tmp/pnpm-global-config"; pnpm_global_store="$pnpm_global/store"
mkdir -p "$pnpm_global" "$pnpm_global_bin_expected" "$pnpm_global_home" "$pnpm_global_config" "$pnpm_global_store"
env -i HOME="$pnpm_global_home" XDG_CONFIG_HOME="$pnpm_global_config" \
  pnpm_config_minimum_release_age=0 \
  PNPM_HOME="$pnpm_global" PNPM_STORE_DIR="$pnpm_global_store" PATH="$pnpm_global_bin_expected:$manager_path" \
  "$pnpm_tool" add --ignore-scripts --global --store-dir "$pnpm_global_store" "@tech-byte-frontier/hardgate@$selector"
pnpm_global_bin=$(env -i HOME="$pnpm_global_home" XDG_CONFIG_HOME="$pnpm_global_config" \
  PNPM_HOME="$pnpm_global" PNPM_STORE_DIR="$pnpm_global_store" PATH="$pnpm_global_bin_expected:$manager_path" \
  "$pnpm_tool" bin --global)
test "$pnpm_global_bin" = "$pnpm_global_bin_expected"
test -d "$pnpm_global_bin"; pnpm_global_command="$pnpm_global_bin/hardgate"
test -x "$pnpm_global_command"
env -i HOME="$pnpm_global_home" EXPECTED="$expected" PATH="$pnpm_global_bin:$private_bin:$node_bin:/usr/bin:/bin" \
  sh -c 'test "$(command -v hardgate)" = "$1" && test "$(hardgate --version)" = "$EXPECTED"' sh "$pnpm_global_command"
verify_install "$pnpm_global" "$pnpm_global_command" "$pnpm_global_home" pnpm-global
