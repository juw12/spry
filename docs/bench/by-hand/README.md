# Readings taken by hand

These files are not from a CI run. They are `spry bench --json` of the released downloads
(0.3.0 and 0.3.1, checked against their `.sha256`), run on the author's Windows PC on 2026-11-19:
8 processor threads, Windows 10, plugged in, and busy with the author's own background jobs at
below-normal priority (about five threads' worth). Spry itself was not installed or running there,
so only the test's own background processes were held.

- `windows-pc-0.3.0-a.json`: the second of two first readings of 0.3.0 (the first was read from the
  screen and not kept: held apps got 48% of their free speed while lent).
- `windows-pc-turn1-0.3.1.json` to `windows-pc-turn4-0.3.0.json`: the two versions in turns, one
  after the other, about 40 seconds each.

`background.rest_share_lent` is what the held processes got while the app in front rested, as a
share of what they get with Spry off.
