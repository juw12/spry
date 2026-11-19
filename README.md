<p align="center"><img src="assets/logo.png" width="96" alt="Spry"></p>

# Spry

**Keeps an old computer quick.** Spry watches which app you are using and holds the busy apps
behind it back, so the one in front gets the processor. When you switch apps, it switches with you.
When Spry stops, everything is exactly as it was.

One small program for **Windows, macOS and Linux**. Free and open source (MIT). No account, no
internet connection needed, no administrator rights.

![The Spry page: what is held back right now, and a rule per app](docs/dashboard.png)

<sub>The real page, shown with example apps.</sub>

## What you get

Measured by the program itself (`spry bench`) on GitHub's test machines, which have 3 or 4 processor
cores like many older laptops. The test starts twice as many busy background processes as the
machine has cores, then measures how much work the app in front gets done.

<!-- bench:start -->
| System | Cores | Quiet | Busy, Spry off | Busy, Spry on | Gain (lowest-highest) | Of quiet speed | Easing only | With pulses | Slowest 5% of steps, off -> on |
|---|---|---|---|---|---|---|---|---|---|
| Windows, x64 | 4 | 250 | 180 | 228 | **1.26x** (1.25-1.27) | 91% | 1.26x | not used | 20.6 -> 5.0 ms |
| macOS 15, Apple silicon | 3 | 220 | 101 | 245 | **2.36x** (2.13-2.46) | 112% | 2.23x | 2.36x | 67.4 -> 73.5 ms |
| macOS 15, Intel | 4 | 256 | 119 | 249 | **2.14x** (2.04-2.96) | 97% | 2.10x | 2.14x | 126.3 -> 109.8 ms |
| Linux (Ubuntu), x64 | 4 | 252 | 89 | 218 | **2.45x** (2.23-2.88) | 87% | 1.09x | 2.45x | 15.0 -> 15.1 ms |

Median of 3 CI runs ([44464015119](https://github.com/juw12/spry/actions/runs/44464015119), [44464540756](https://github.com/juw12/spry/actions/runs/44464540756), [44465145704](https://github.com/juw12/spry/actions/runs/44465145704)). Work is in steps per second.
<!-- bench:end -->

- **Quiet** = nothing else running. **Busy, Spry off** = background processes take what they want.
  **Busy, Spry on** = the same load, held back by Spry (the better of "easing only" and "with pulses").
- **Easing only** is the default on Windows and macOS. **With pulses** is "Strong mode": the default on
  Linux, one tick on the page elsewhere.
- **Slowest 5% of steps**: how long a short piece of work takes after a pause, like reacting to a
  click. On the two macOS test machines this did not improve; the amount of work did.
- These are shared virtual machines and the numbers move between runs, which is why the lowest and
  highest gain are shown. Your computer will differ. Measure it: `spry bench`, or "Measure now" on the page.

Spry itself uses about 8 MB of memory and 0.1-0.3% of the processor (measured on Windows with
370 processes running: 7.7 MB, 2.2% of one of 8 cores).

## What it cannot do

- It does not add memory or make the processor faster. It decides who goes first. Work in the
  background takes longer while you use something else, and gets its speed back when you stop.
- If the app in front is slow by itself with nothing else running, Spry changes nothing.
- A computer that is out of memory stays out of memory. On Windows, Spry can ask large background
  apps to give back memory they are not using; on macOS and Linux the system does not offer that.
- The downloads are not signed yet, so Windows and macOS warn on first start (see Install).

## Install

**Windows** (PowerShell):

```powershell
irm https://raw.githubusercontent.com/juw12/spry/main/install.ps1 | iex
```

**macOS and Linux** (Terminal):

```sh
curl -fsSL https://raw.githubusercontent.com/juw12/spry/main/install.sh | sh
```

Or download the file for your system from [Releases](https://github.com/juw12/spry/releases/latest),
unpack it and start `spry`. Both scripts put one file in your user folder and start it; nothing is
installed system-wide.

Spry then sits in the tray (Windows) or menu bar (macOS) and opens its page in your browser. On Linux
there is no tray icon: run `spry open` for the page. Tick "Start Spry when I sign in" to keep it.

To remove it: `spry uninstall`, quit Spry, delete the file.

## How it works

Every half second Spry asks the system which app is in front. Every two seconds (every five while
the computer is calm) it measures all running apps.

- **The app in front is never touched**, together with everything it started and the app that
  started it.
- An app you just left keeps full speed for 10 seconds.
- A background app that uses more than 10% of one processor core is **eased**: it only gets the
  processor when the app in front does not need it.
- When the whole processor is above 80% for 4 seconds, easing goes deeper, and Strong mode also
  **pulses** the heaviest background processes (120 ms paused, 80 ms running).
- You can set a rule per app: never slow, always slow in the background, or **freeze** in the
  background (fully paused until you switch back to it).
- Calls, music, recording, the desktop and the system are on a built-in "never touched" list.

What "eased" means on each system:

| System | Eased | Notes |
|---|---|---|
| Windows 10/11 | Lower priority class. Under pressure also Efficiency mode and low memory priority. | Also releases unused memory of large background apps when memory runs low. |
| macOS 11+ | The system's background state (what `taskpolicy -b` sets): lowest processor priority, throttled disk. | Apple silicon and Intel. |
| Linux | Idle disk priority, and pulses under pressure. | A normal user cannot raise a priority again after lowering it, so Spry does not lower it. Needs X11 to see the app in front; on Wayland Spry only follows the rules you set. |

## Safety

- **Everything is put back** when you pause Spry, switch it off or quit it. Spry saves what each
  process looked like before, and restores exactly that.
- If Spry is killed or the computer crashes, the list of held processes is on disk: the next start,
  or `spry rescue`, puts them back. A frozen app is resumed the same way.
- If an app changes its own priority (browsers do this for their tabs), Spry leaves that app alone.
- Nothing needs administrator rights, so Spry cannot touch system processes or other users' apps.
- The page is served on `127.0.0.1` only and needs a secret token that is created on your computer.
- Storage cleaning shows what it found and removes nothing until you choose. It only looks at
  caches, temporary files and the bin, never at documents, downloads or app data.

This is tested on every change, on Windows, macOS (Apple silicon and Intel) and Linux
([CI](https://github.com/juw12/spry/actions/workflows/ci.yml)): a real busy process is eased,
paused, frozen, and put back; Spry is killed and `spry rescue` puts the process back.

## Commands

```
spry              start Spry and open its page
spry open         open the page of the running Spry
spry status       show what is being held back right now
spry bench        measure the effect on this computer (about 20 seconds)
spry clean        show what storage can be cleared; add --yes to clear it
spry install      start Spry when you sign in      (spry uninstall undoes it)
spry rescue       put processes back after a crash
```

On Windows, Spry is a windowed program, so in a terminal add `| more` to read the output of
`spry status` or `spry bench`.

## Other tools

Spry is the only one of these that runs on all three systems.

| Tool | System | Price |
|---|---|---|
| [Process Lasso](https://bitsum.com/) | Windows | Free version; Pro from $39.95 for one PC ([Bitsum](https://bitsum.com/get-lasso-pro/), checked 2026-11-19) |
| [App Tamer](https://www.stclairsoft.com/AppTamer/) | macOS | Paid, see the maker's page |
| [ananicy-cpp](https://gitlab.com/ananicy-cpp/ananicy-cpp), [cpulimit](https://github.com/opsengine/cpulimit) | Linux | Free, open source, set up by hand |
| **Spry** | Windows, macOS, Linux | Free, open source |

Process Lasso and App Tamer are older and have more settings. Spry is for people who want it to
work without settings.

## Build from source

```sh
cargo build --release      # Rust 1.88 or newer
cargo test --release       # starts real busy processes for a few seconds
```

The numbers in "What you get" are rebuilt from the CI results in `docs/bench/` with
`python docs/bench/summary.py`.

## Licence

MIT. Made by [Juwen Lie](https://github.com/juw12).
