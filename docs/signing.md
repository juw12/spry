# Signed downloads: what is needed

Today Spry's downloads are not signed by a certificate, so Windows and macOS warn on first start.
This page says what has to be bought, what is already prepared, and what is not tested.
Prices were read on 20 Nov 2026 and can change.

## macOS: prepared, not tested

What to buy: the **Apple Developer Program**, $99 a year
([developer.apple.com/programs](https://developer.apple.com/programs/)). It gives a
"Developer ID Application" certificate and lets Apple check each release (notarization). After
that a Mac opens Spry with one ordinary question instead of the four install steps.

What is prepared: the release workflow has a step that signs the program and the app, sends both
to Apple, and fastens Apple's answer to the app (`scripts/sign-macos.sh`). The step runs only
when the repository has these five secrets. Without them it is skipped and the downloads are
built as before.

| Secret (Settings > Secrets and variables > Actions) | What goes in |
|---|---|
| `MACOS_CERT_P12` | The Developer ID Application certificate with its key, exported from Keychain Access as a .p12 file, then `base64 -i file.p12` |
| `MACOS_CERT_PASSWORD` | The password chosen at the export |
| `APPLE_ID` | The Apple account's e-mail address |
| `APPLE_TEAM_ID` | The team ID shown under Membership on developer.apple.com |
| `APPLE_APP_PASSWORD` | An app-specific password made at account.apple.com for this purpose |

**Not tested.** No certificate has been bought, so the signing step has never run. The first time:

1. Add the secrets, then start the release workflow by hand (Actions > release > Run workflow).
   Started by hand it builds and signs but publishes nothing.
2. Read the log of the signing step. Its last lines are what a Mac will say about the app.
3. Signing switches on Apple's "hardened runtime". Check on a real Mac that the signed Spry still
   sees the app in front (`spry front`) and still holds a busy app. If it does not, the program
   needs an entitlement and the script needs one more argument.
4. Then update the install steps in the README: steps 3 and 4 fall away.

## Windows: a decision first

Since June 2023 the key of a new code-signing certificate has to live on a hardware token or in
the seller's cloud service; it is no longer a file that can be put into a secret. (This comes from
a search and was not read at the source: ask the seller.) So the signing step depends on where
the certificate comes from, and none is prepared yet. The choices found:

| Choice | Price | For Spry |
|---|---|---|
| [SignPath Foundation](https://signpath.org/terms.html) | Free for open-source projects | Read on its terms page: the licence must be open source "without commercial dual-licensing", with no proprietary part. The certificate names SignPath Foundation as publisher, not the author. Whether a project that also sells a ready-to-run copy is accepted is not stated there: ask before applying. |
| [Certum Open Source Code Signing](https://certum.store/open-source-code-signing-on-simplysign.html), key in Certum's cloud | $58.00 on the shop page | For an individual open-source developer. Signing asks for a code from a phone app, so it is done by hand for each release, not by the release workflow. Not tried. |
| Microsoft Artifact Signing | about $9.99 a month | For individuals in the USA and Canada only. |
| An ordinary (OV) certificate from another seller | $150 to $300 a year according to Microsoft's overview | Works from a hardware token or the seller's cloud service. |

Whichever is chosen: a new certificate does not remove the Windows SmartScreen warning at once.
Microsoft's overview says reputation builds over time and warnings are expected at first.

**Assumed** until decided otherwise: ask SignPath Foundation first, because it costs nothing and
Spry stays MIT. If Spry is sold as a signed copy and they decline, take the Certum certificate.

## What stays true without any certificate

Every download has a checksum and a signed build record that says which commit and which workflow
made it (`gh attestation verify <file> -R juw12/spry`). The README explains both under
"Check a download".
