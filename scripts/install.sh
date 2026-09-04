#!/usr/bin/env sh
# Install latest: curl -fsSL https://raw.githubusercontent.com/Tech-Byte-Frontier/hardgate/main/scripts/install.sh | sh
# Install a release: curl -fsSL .../scripts/install.sh | HARDGATE_VERSION=vX.Y.Z sh
# HARDGATE_VERSION accepts `vX.Y.Z` or `X.Y.Z` (latest when omitted).
# HARDGATE_INSTALL_DIR overrides the default `$HOME/.cargo/bin` destination.
# HARDGATE_LIBC=gnu|musl overrides Linux libc detection when a host is
# intentionally minimal and neither `getconf` nor `ldd` can identify it.
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
  case "${HARDGATE_LIBC:-}" in
    gnu|glibc) libc_suffix="" ;;
    musl) libc_suffix="-musl" ;;
    "")
      if [ -f /etc/alpine-release ]; then
        libc_suffix="-musl"
      elif command -v getconf >/dev/null 2>&1 && getconf GNU_LIBC_VERSION >/dev/null 2>&1; then
        libc_suffix=""
      elif command -v ldd >/dev/null 2>&1 && ldd --version 2>&1 | grep -qi musl; then
        libc_suffix="-musl"
      else
        musl_loader=0
        for loader in /lib/ld-musl-*.so.1 /lib64/ld-musl-*.so.1 /usr/lib/ld-musl-*.so.1; do
          if [ -e "$loader" ]; then musl_loader=1; break; fi
        done
        if [ "$musl_loader" -eq 1 ]; then
          libc_suffix="-musl"
        else
          echo "hardgate: cannot determine Linux libc; set HARDGATE_LIBC=gnu or HARDGATE_LIBC=musl" >&2
          exit 1
        fi
      fi
      ;;
    *) echo "hardgate: HARDGATE_LIBC must be gnu, glibc, or musl" >&2; exit 1 ;;
  esac
fi

case "${os}-${arch}" in
  linux-x86_64)
    pkg="hardgate-linux-x64${libc_suffix}"
    target="x86_64-unknown-linux-${libc_suffix#-}"
    [ "$libc_suffix" = "" ] && target="x86_64-unknown-linux-gnu"
    ;;
  linux-aarch64|linux-arm64)
    pkg="hardgate-linux-arm64${libc_suffix}"
    target="aarch64-unknown-linux-${libc_suffix#-}"
    [ "$libc_suffix" = "" ] && target="aarch64-unknown-linux-gnu"
    ;;
  darwin-x86_64) pkg="hardgate-darwin-x64"; target="x86_64-apple-darwin" ;;
  darwin-aarch64|darwin-arm64) pkg="hardgate-darwin-arm64"; target="aarch64-apple-darwin" ;;
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
staging_dir=""
staged=""
cleanup() {
  rm -rf "$tmp"
  if [ -n "$staging_dir" ]; then rm -rf "$staging_dir"; fi
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
  expected=$(printf '%s\n' "$checksum_line" | awk '{print $1}' | tr '[:upper:]' '[:lower:]')
  if command -v sha256sum >/dev/null 2>&1; then
    # BusyBox accepts the basic digest invocation but not GNU's
    # `--check --status` flags. Compare the exact digest instead of relying on
    # implementation-specific verification options.
    actual=$(sha256sum "$tmp/$archive_name" | awk '{print $1}' | tr '[:upper:]' '[:lower:]')
    [ "$actual" = "$expected" ]
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

archive_members=$(tar tzf "$tmp/$archive_name" | sort)
expected_members=$(printf '%s\n' "$pkg/" "$pkg/BUILD-METADATA.json" "$pkg/hardgate" | sort)
if [ "$archive_members" != "$expected_members" ]; then
  echo "hardgate: archive members do not exactly match ${pkg}/ package" >&2
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
metadata_version=$(printf '%s\n' "$metadata" | sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
metadata_commit=$(printf '%s\n' "$metadata" | sed -n 's/.*"commit"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
metadata_name=$(printf '%s\n' "$metadata" | sed -n 's/.*"name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
metadata_package=$(printf '%s\n' "$metadata" | sed -n 's/.*"package"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
metadata_target=$(printf '%s\n' "$metadata" | sed -n 's/.*"target"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
if [ -z "$metadata_version" ] || ! printf '%s\n' "$metadata_version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-.+][0-9A-Za-z.-]+)?$'; then
  echo "hardgate: archive metadata has no valid release version" >&2
  exit 1
fi
if [ "$metadata_name" != "hardgate" ] || [ "$metadata_package" != "$pkg" ] || [ "$metadata_target" != "$target" ]; then
  echo "hardgate: archive metadata target/package does not match ${target}/${pkg}" >&2
  exit 1
fi
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
staging_dir=$(mktemp -d "$INSTALL_DIR/.hardgate.XXXXXX") || {
  echo "hardgate: cannot create a private staging directory in ${INSTALL_DIR}" >&2
  exit 1
}
staged="$staging_dir/hardgate"
cp "$binary" "$staged"
chmod 755 "$staged"
installed_version=$("$staged" --version)
installed_name_version=$(printf '%s\n' "$installed_version" | sed -n 's/^hardgate \([^ ]*\) (\([0-9a-fA-F]*\))$/\1/p')
installed_commit=$(printf '%s\n' "$installed_version" | sed -n 's/^hardgate \([^ ]*\) (\([0-9a-fA-F]*\))$/\2/p')
if [ "$installed_name_version" != "$metadata_version" ]; then
  echo "hardgate: installed binary version ${installed_name_version:-<missing>} does not match archive ${metadata_version}" >&2
  exit 1
fi
if [ "$installed_commit" != "$metadata_commit" ]; then
  echo "hardgate: installed binary commit ${installed_commit:-<missing>} does not match archive ${metadata_commit}" >&2
  exit 1
fi
if [ "$installed_version" != "hardgate ${metadata_version} (${metadata_commit})" ]; then
  echo "hardgate: installed binary returned an identity different from archive metadata" >&2
  exit 1
fi
mv -f "$staged" "$INSTALL_DIR/hardgate"
staged=""
rmdir "$staging_dir"
staging_dir=""
echo "hardgate: installed to ${INSTALL_DIR}/hardgate (${installed_version})"
