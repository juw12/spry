#!/bin/sh
# Signs Spry with an Apple Developer ID and has Apple check it (notarization), so a Mac opens
# the download without the "could not verify" question.
#   sh scripts/sign-macos.sh <spry binary> <Spry.app> <tar.gz to write> <zip to write>
# The release workflow runs this only when the repository has the secrets named in
# docs/signing.md. NOT TESTED YET: no certificate has been bought, so this has never run.
set -eu

binary="$1"
app="$2"
tarball="$3"
zip="$4"
work="${RUNNER_TEMP:-$(mktemp -d)}"
keychain="$work/spry-signing.keychain-db"
keychain_password="$(uuidgen)"

# The certificate goes into a keychain of its own, which is deleted at the end.
printf '%s' "$P12" | base64 --decode > "$work/developer-id.p12"
security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 3600 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
security import "$work/developer-id.p12" -P "$P12_PASSWORD" -t cert -f pkcs12 -k "$keychain" -T /usr/bin/codesign
rm -f "$work/developer-id.p12"
security set-key-partition-list -S apple-tool:,apple: -s -k "$keychain_password" "$keychain" > /dev/null
# shellcheck disable=SC2046
security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')
trap 'security delete-keychain "$keychain"' EXIT

identity="$(security find-identity -v -p codesigning "$keychain" | awk -F'"' '/Developer ID Application/ { print $2; exit }')"
[ -n "$identity" ] || { echo "MACOS_CERT_P12 holds no Developer ID Application certificate" >&2; exit 1; }

apple_checks() { # send one file to Apple and wait for the answer
  xcrun notarytool submit "$1" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD" --wait
}

# The program by itself (the tar.gz for the one-line install).
codesign --force --options runtime --timestamp --sign "$identity" "$binary"
codesign --verify --strict --verbose=2 "$binary"
ditto -c -k "$binary" "$work/spry-program.zip"
apple_checks "$work/spry-program.zip"
tar -czf "$tarball" "$binary"

# The app: the signed program goes in, the app is signed around it, Apple checks it, and the
# answer is fastened to the app so it also opens without the internet.
cp "$binary" "$app/Contents/MacOS/spry"
codesign --force --options runtime --timestamp --sign "$identity" "$app"
codesign --verify --deep --strict --verbose=2 "$app"
ditto -c -k --keepParent "$app" "$zip"
apple_checks "$zip"
xcrun stapler staple "$app"
ditto -c -k --keepParent "$app" "$zip"

# What a Mac will say when someone opens it.
spctl --assess --type execute --verbose=2 "$app"
"$app/Contents/MacOS/spry" version
