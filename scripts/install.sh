#!/usr/bin/env sh
# Install latest: curl -fsSL https://raw.githubusercontent.com/Tech-Byte-Frontier/hardgate/main/scripts/install.sh | sh
# Install a release: curl -fsSL .../scripts/install.sh | HARDGATE_VERSION=v0.4.3 sh
# HARDGATE_VERSION accepts `vX.Y.Z` or `X.Y.Z` (latest when omitted).
# HARDGATE_INSTALL_DIR overrides the default `$HOME/.cargo/bin` destination.
set -eu

REPO="Tech-Byte-Frontier/hardgate"
REQUESTED_VERSION="${HARDGATE_VERSION:-latest}"
if [ -n "${HARDGATE_INSTALL_DIR:-}" ]; then
  INSTALL_DIR="$HARDGATE_INSTALL_DIR"
else
  HOME_DIR="${HOME:-}"
  if [ -z "$HOME_DIR" ]; then
    echo "hardgate: HOME is required unless HARDGATE_INSTALL_DIR is set" >&2
    exit 1
  fi
  INSTALL_DIR="$HOME_DIR/.cargo/bin"
fi

os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)
libc_suffix=""
if [ "$os" = "linux" ]; then
  if [ -f /etc/alpine-release ] || { command -v getconf >/dev/null 2>&1 && ! getconf GNU_LIBC_VERSION >/dev/null 2>&1; }; then
    libc_suffix="-musl"
  fi
fi

case "${os}-${arch}" in
  linux-x86_64) pkg="hardgate-linux-x64${libc_suffix}" ;;
  linux-aarch64|linux-arm64) pkg="hardgate-linux-arm64${libc_suffix}" ;;
  darwin-x86_64) pkg="hardgate-darwin-x64" ;;
  darwin-aarch64|darwin-arm64) pkg="hardgate-darwin-arm64" ;;
  *) echo "hardgate: unsupported platform ${os}/${arch}" >&2; exit 1 ;;
esac

case "$REQUESTED_VERSION" in
  latest) release_ref="latest"; expected_version="" ;;
  v[0-9]*|[0-9]*)
    case "$REQUESTED_VERSION" in
      v*) release_ref="$REQUESTED_VERSION"; expected_version="${REQUESTED_VERSION#v}" ;;
      *) release_ref="v$REQUESTED_VERSION"; expected_version="$REQUESTED_VERSION" ;;
    esac
    if ! printf '%s\n' "$expected_version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-.+][0-9A-Za-z.-]+)?$'; then
      echo "hardgate: HARDGATE_VERSION must be vX.Y.Z or X.Y.Z" >&2
      exit 1
    fi
    ;;
  *) echo "hardgate: HARDGATE_VERSION must be latest, vX.Y.Z, or X.Y.Z" >&2; exit 1 ;;
esac

if [ "$release_ref" = "latest" ]; then
  base_url="https://github.com/${REPO}/releases/latest/download"
else
  base_url="https://github.com/${REPO}/releases/download/${release_ref}"
fi
archive_name="${pkg}.tar.gz"
archive_url="${base_url}/${archive_name}"
checksum_url="${base_url}/SHA256SUMS"

echo "hardgate: downloading ${archive_url}"
tmp=$(mktemp -d)
staged=""
cleanup() {
  rm -rf "$tmp"
  if [ -n "$staged" ]; then rm -f "$staged"; fi
}
trap cleanup EXIT
trap 'exit 130' HUP INT TERM
curl --fail --location --silent --show-error "$checksum_url" -o "$tmp/SHA256SUMS"
checksum_line=$(awk -v wanted="$archive_name" '$2 == wanted { line=$0; count++ } END { if (count != 1) exit 1; print line }' "$tmp/SHA256SUMS") || {
  echo "hardgate: SHA256SUMS has no unique entry for ${archive_name}" >&2
  exit 1
}
curl --fail --location --silent --show-error "$archive_url" -o "$tmp/$archive_name"
verify_checksum() {
  expected=$(printf '%s\n' "$checksum_line" | awk '{print $1}')
  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$tmp" && printf '%s\n' "$checksum_line" | sha256sum --check --status -)
  elif command -v shasum >/dev/null 2>&1; then
    actual=$(shasum -a 256 "$tmp/$archive_name" | awk '{print $1}')
    [ "$actual" = "$expected" ]
  else
    echo "hardgate: sha256sum or shasum is required for checksum verification" >&2
    return 1
  fi
}
verify_checksum || {
  echo "hardgate: checksum verification failed for ${archive_name}" >&2
  exit 1
}

if ! tar tzf "$tmp/$archive_name" | awk -v root="$pkg/" '$0 == root { found=1 } END { exit(found ? 0 : 1) }'; then
  echo "hardgate: archive has no ${pkg}/ root" >&2
  exit 1
fi
tar xzf "$tmp/$archive_name" -C "$tmp"
binary="$tmp/$pkg/hardgate"
if [ ! -f "$binary" ]; then
  echo "hardgate: archive has no hardgate binary" >&2
  exit 1
fi
metadata=$(tar xOf "$tmp/$archive_name" "$pkg/BUILD-METADATA.json") || {
  echo "hardgate: archive metadata missing" >&2
  exit 1
}
metadata_version=$(printf '%s\n' "$metadata" | sed -n 's/.*"version":"\([^"]*\)".*/\1/p')
metadata_commit=$(printf '%s\n' "$metadata" | sed -n 's/.*"commit":"\([^"]*\)".*/\1/p')
commit_length=$(printf '%s' "$metadata_commit" | awk '{print length}')
if [ -z "$metadata_commit" ] || [ "$metadata_commit" = unknown ] || ! printf '%s\n' "$metadata_commit" | grep -Eq '^[0-9a-fA-F]+$' || { [ "$commit_length" -ne 40 ] && [ "$commit_length" -ne 64 ]; }; then
  echo "hardgate: archive metadata has no full source commit identity" >&2
  exit 1
fi
if [ -n "$expected_version" ]; then
  if [ "$metadata_version" != "$expected_version" ]; then
    echo "hardgate: archive version does not match ${expected_version}" >&2
    exit 1
  fi
fi

mkdir -p "$INSTALL_DIR"
staged="$INSTALL_DIR/.hardgate.$$"
cp "$binary" "$staged"
chmod 755 "$staged"
installed_version=$("$staged" --version)
installed_commit=$(printf '%s\n' "$installed_version" | sed -n 's/^hardgate [^ ]* (\([0-9a-fA-F]*\))$/\1/p')
if [ "$installed_commit" != "$metadata_commit" ]; then
  echo "hardgate: installed binary commit ${installed_commit:-<missing>} does not match archive ${metadata_commit}" >&2
  exit 1
fi
if [ -n "$expected_version" ]; then
  case "$installed_version" in
    "hardgate ${expected_version} ("*) ;;
    *) echo "hardgate: installed binary reports ${installed_version}, expected hardgate ${expected_version}" >&2; exit 1 ;;
  esac
else
  case "$installed_version" in
    "hardgate "*) ;;
    *) echo "hardgate: installed binary returned an invalid version" >&2; exit 1 ;;
  esac
fi
mv -f "$staged" "$INSTALL_DIR/hardgate"
echo "hardgate: installed to ${INSTALL_DIR}/hardgate (${installed_version})"
