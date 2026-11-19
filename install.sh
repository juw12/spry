#!/bin/sh
# Installs Spry for the current user on macOS or Linux. No administrator rights needed.
#   curl -fsSL https://raw.githubusercontent.com/juw12/spry/main/install.sh | sh
set -eu

case "$(uname -s)" in
  Darwin) asset="spry-macos-universal.tar.gz" ;;
  Linux)
    case "$(uname -m)" in
      x86_64 | amd64) asset="spry-linux-x64.tar.gz" ;;
      aarch64 | arm64) asset="spry-linux-arm64.tar.gz" ;;
      *) echo "No ready-made Spry for $(uname -m) yet. Build it with: cargo install --git https://github.com/juw12/spry" >&2; exit 1 ;;
    esac ;;
  *) echo "This installer is for macOS and Linux. On Windows use install.ps1." >&2; exit 1 ;;
esac

dir="${SPRY_INSTALL_DIR:-$HOME/.local/bin}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

from="${SPRY_FROM:-https://github.com/juw12/spry/releases/latest/download}"

# The download must match the checksum published next to it, or nothing is installed.
fetch_checked() {
  curl -fsSL "$from/$1" -o "$tmp/$1" || return 1
  curl -fsSL "$from/$1.sha256" -o "$tmp/$1.sha256" || return 1
  want="$(cut -d' ' -f1 "$tmp/$1.sha256")"
  if command -v sha256sum >/dev/null 2>&1; then
    got="$(sha256sum "$tmp/$1" | cut -d' ' -f1)"
  elif command -v shasum >/dev/null 2>&1; then
    got="$(shasum -a 256 "$tmp/$1" | cut -d' ' -f1)"
  else
    echo "Cannot check the download: neither sha256sum nor shasum is on this system. Nothing was installed." >&2
    exit 1
  fi
  if [ -z "$want" ] || [ "$want" != "$got" ]; then
    echo "The download does not match its published checksum. Nothing was installed." >&2
    exit 1
  fi
  echo "Checksum matches."
}

# On a Mac Spry is an app: Spry.app in Applications, with the spry command pointing into it.
# A release from before 0.4 has no app; then the plain program is installed as before.
if [ "$(uname -s)" = Darwin ]; then
  echo "Downloading Spry-macos.zip ..."
  if fetch_checked Spry-macos.zip; then
    if [ -n "${SPRY_INSTALL_DIR:-}" ]; then
      apps="$SPRY_INSTALL_DIR"
    elif [ -w /Applications ]; then
      apps="/Applications"
    else
      apps="$HOME/Applications"
    fi
    ditto -x -k "$tmp/Spry-macos.zip" "$tmp/app"
    mkdir -p "$apps" "$dir"
    # Stop a running copy first, so it can be replaced and every app is put back.
    pkill -x spry 2>/dev/null && sleep 1 || true
    rm -rf "$apps/Spry.app"
    mv "$tmp/app/Spry.app" "$apps/Spry.app"
    rm -f "$dir/spry"
    ln -s "$apps/Spry.app/Contents/MacOS/spry" "$dir/spry"
    echo "Spry $("$dir/spry" version | cut -d' ' -f2) is in $apps/Spry.app"
    case ":$PATH:" in
      *":$dir:"*) ;;
      *) echo "To use the spry command in a terminal, add $dir to your PATH." ;;
    esac
    if [ -n "${SPRY_NO_START:-}" ]; then
      echo "Start it from Applications, or with:  open \"$apps/Spry.app\""
      exit 0
    fi
    echo "Starting it now. To start it at every sign-in, tick the box under Settings in its window."
    open "$apps/Spry.app"
    exit 0
  fi
  echo "This release has no Mac app yet; installing the plain program."
fi

echo "Downloading $asset ..."
if ! fetch_checked "$asset"; then
  echo "Could not download $from/$asset. Nothing was installed." >&2
  exit 1
fi

tar -xzf "$tmp/$asset" -C "$tmp"
mkdir -p "$dir"
# Stop a running copy first, so the file can be replaced and every app is put back.
if [ -x "$dir/spry" ]; then pkill -x spry 2>/dev/null || true; sleep 1; fi
mv -f "$tmp/spry" "$dir/spry"
chmod +x "$dir/spry"

echo "Spry $("$dir/spry" version | cut -d' ' -f2) is in $dir/spry"
case ":$PATH:" in
  *":$dir:"*) ;;
  *) echo "To use the spry command in a terminal, add $dir to your PATH." ;;
esac
if [ -n "${SPRY_NO_START:-}" ]; then
  echo "Start it with:  $dir/spry"
  exit 0
fi
echo "Starting it now. To start it at every sign-in, tick the box on its page."
# In the background and cut loose from this terminal, so closing the window does not stop it.
nohup "$dir/spry" >/dev/null 2>&1 &
