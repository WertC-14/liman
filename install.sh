#!/bin/sh
# Installs the latest liman release for this machine into ~/.local/bin (or $LIMAN_INSTALL_DIR).
#
#   curl -fsSL https://raw.githubusercontent.com/WertC-14/liman/main/install.sh | sh
#
# Static binaries: no Rust, no libraries needed on the server.
set -eu

repo="WertC-14/liman"
dir="${LIMAN_INSTALL_DIR:-$HOME/.local/bin}"

case "$(uname -s)" in
  Linux) ;;
  *) echo "liman: only Linux binaries are published; build from source with: cargo install --git https://github.com/$repo liman" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) target="x86_64-unknown-linux-musl" ;;
  aarch64 | arm64) target="aarch64-unknown-linux-musl" ;;
  *) echo "liman: no binary for $(uname -m)" >&2; exit 1 ;;
esac

if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -qO "$2" "$1"; }
else
  echo "liman: needs curl or wget" >&2; exit 1
fi

url="https://github.com/$repo/releases/latest/download/liman-$target.tar.gz"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
echo "Downloading $url"
fetch "$url" "$tmp/liman.tar.gz"
fetch "$url.sha256" "$tmp/liman.tar.gz.sha256" 2>/dev/null || true
if [ -s "$tmp/liman.tar.gz.sha256" ] && command -v sha256sum >/dev/null 2>&1; then
  expected="$(cut -d' ' -f1 "$tmp/liman.tar.gz.sha256")"
  actual="$(sha256sum "$tmp/liman.tar.gz" | cut -d' ' -f1)"
  [ "$expected" = "$actual" ] || { echo "liman: checksum mismatch" >&2; exit 1; }
fi
tar -xzf "$tmp/liman.tar.gz" -C "$tmp"
bin="$(find "$tmp" -type f -name liman | head -n 1)"
[ -n "$bin" ] || { echo "liman: binary not found in the archive" >&2; exit 1; }
mkdir -p "$dir"
install -m 755 "$bin" "$dir/liman"
echo "Installed $("$dir/liman" --version) to $dir/liman"
case ":$PATH:" in
  *":$dir:"*) ;;
  *) echo "Add $dir to your PATH, e.g.: export PATH=\"$dir:\$PATH\"" ;;
esac
