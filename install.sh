#!/bin/sh
# Installs Spry for the current user on macOS or Linux. No administrator rights needed.
#   curl -fsSL https://raw.githubusercontent.com/juw12/spry/main/install.sh | sh
set -eu

case "$(uname -s)" in
  Darwin) asset="spry-macos-universal.tar.gz" ;;
  Linux)
    case "$(uname -m)" in
      x86_64 | amd64) asset="spry-linux-x64.tar.gz" ;;
      *) echo "No ready-made Spry for $(uname -m) yet. Build it with: cargo install --git https://github.com/juw12/spry" >&2; exit 1 ;;
    esac ;;
  *) echo "This installer is for macOS and Linux. On Windows use install.ps1." >&2; exit 1 ;;
esac

dir="${SPRY_INSTALL_DIR:-$HOME/.local/bin}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "Downloading $asset ..."
curl -fsSL "https://github.com/juw12/spry/releases/latest/download/$asset" -o "$tmp/spry.tar.gz"
tar -xzf "$tmp/spry.tar.gz" -C "$tmp"
mkdir -p "$dir"
# Stop a running copy first, so the file can be replaced and every app is put back.
if [ -x "$dir/spry" ]; then pkill -x spry 2>/dev/null || true; sleep 1; fi
mv -f "$tmp/spry" "$dir/spry"
chmod +x "$dir/spry"

echo "Spry $("$dir/spry" version | cut -d' ' -f2) is in $dir/spry"
case ":$PATH:" in
  *":$dir:"*) echo "Start it with:  spry" ;;
  *) echo "Start it with:  $dir/spry" ;;
esac
echo "To start it at every sign-in:  $dir/spry install"
