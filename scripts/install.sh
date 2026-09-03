#!/usr/bin/env sh
# hardgate one-liner: curl -fsSL https://raw.githubusercontent.com/Tech-Byte-Frontier/hardgate/main/scripts/install.sh | sh
# Installs the prebuilt binary from GitHub Releases into $HOME/.cargo/bin (or $ hardgate_INSTALL_DIR).
set -eu

REPO="Tech-Byte-Frontier/hardgate"
VERSION="${HARDGATE_VERSION:-latest}"
INSTALL_DIR="${HARDGATE_INSTALL_DIR:-$HOME/.cargo/bin}"

os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)
libc_suffix=""
if [ "$os" = "linux" ]; then
  if [ -f /etc/alpine-release ] || ! getconf GNU_LIBC_VERSION >/dev/null 2>&1; then
    libc_suffix="-musl"
  fi
fi

case "${os}-${arch}" in
  linux-x86_64) pkg="hardgate-linux-x64${libc_suffix}" ;;
  linux-aarch64|linux-arm64) pkg="hardgate-linux-arm64${libc_suffix}" ;;
  darwin-x86_64) pkg="hardgate-darwin-x64" ;;
  darwin-aarch64|darwin-arm64) pkg="hardgate-darwin-arm64" ;;
  mingw*-x86_64|msys*-x86_64|cygwin*-x86_64|windows-x86_64) pkg="hardgate-win32-x64" ;;
  *) echo "hardgate: unsupported platform ${os}/${arch}" >&2; exit 1 ;;
esac

if [ "$VERSION" = "latest" ]; then
  URL="https://github.com/${REPO}/releases/latest/download/${pkg}.tar.gz"
else
  URL="https://github.com/${REPO}/releases/download/${VERSION}/${pkg}.tar.gz"
fi

echo "hardgate: downloading ${URL}"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -fsSL "$URL" -o "$tmp/pkg.tar.gz"
tar xzf "$tmp/pkg.tar.gz" -C "$tmp"
mkdir -p "$INSTALL_DIR"
if echo "$pkg" | grep -q win32; then
  cp "$tmp/$pkg/hardgate.exe" "$INSTALL_DIR/hardgate.exe"
else
  cp "$tmp/$pkg/hardgate" "$INSTALL_DIR/hardgate"
  chmod +x "$INSTALL_DIR/hardgate"
fi
echo "hardgate: installed to ${INSTALL_DIR}/hardgate"
"$INSTALL_DIR/hardgate" --version
