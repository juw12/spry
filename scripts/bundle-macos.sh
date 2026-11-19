#!/bin/sh
# Wraps a built spry binary as Spry.app, the form a Mac expects an app to have.
#   sh scripts/bundle-macos.sh <spry binary> [output folder, default dist]
# The binary inside is the same program: Spry.app/Contents/MacOS/spry also answers "status", "bench" and the rest.
set -eu

binary="$1"
out="${2:-dist}"
root="$(cd "$(dirname "$0")/.." && pwd)"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -n 1)"
app="$out/Spry.app"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/spry"
chmod +x "$app/Contents/MacOS/spry"
cp "$root/assets/Spry.icns" "$app/Contents/Resources/Spry.icns"

# LSUIElement: Spry starts as a menu-bar icon with no place in the Dock. It takes one only while its
# window is open (tray.rs), so a start at sign-in shows nothing but the icon.
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Spry</string>
<key>CFBundleDisplayName</key><string>Spry</string>
<key>CFBundleIdentifier</key><string>dev.juw12.spry</string>
<key>CFBundleExecutable</key><string>spry</string>
<key>CFBundleIconFile</key><string>Spry</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
<key>CFBundleShortVersionString</key><string>$version</string>
<key>CFBundleVersion</key><string>$version</string>
<key>LSMinimumSystemVersion</key><string>11.0</string>
<key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
<key>LSUIElement</key><true/>
<key>NSHighResolutionCapable</key><true/>
<key>NSHumanReadableCopyright</key><string>MIT licence. github.com/juw12/spry</string>
</dict></plist>
PLIST

# Not signed by Apple yet: signed "ad hoc", which Apple silicon needs before it runs anything.
codesign --force --deep --sign - "$app"
echo "$app (Spry $version)"
