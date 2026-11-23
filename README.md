<p align="center"><img src="assets/logo.png" width="96" alt="Spry"></p>

# Spry

**Makes an old computer use less, and keeps it quick.** Spry watches which app you are using and
holds the busy apps behind it back. It has two modes:

- **Saver** (the default): busy apps stay held for as long as they are in the background, also while
  you read, think or step away. Where the hold bites, the computer does less work, so it uses less
  processor and stays cooler, and work in the background takes longer. How much is measured under
  [Saver and Performance](#saver-and-performance): on Windows and Linux test machines with four
  processor threads, two busy background apps went from half of the processor to a quarter and got
  half of their work done; on a real MacBook Pro 2019 the computer went from 72% busy to 39%
  (measured with 0.5.0); on the two macOS test machines it saved little or nothing.
- **Performance**: busy background apps are held only while the app in front is working, and get
  the processor back while it rests. The app in front is quicker; the computer does about as much
  work as without Spry.

When you switch apps, it switches with you. When Spry stops, everything is exactly as it was.

One small program for **Windows, macOS and Linux**. Free and open source (MIT). No account, no
administrator rights. It works without an internet connection; with one, it keeps itself up to date
(see [Updates](#updates)).

![Spry's window in Saver mode: what is held back right now](docs/dashboard.png)

<sub>Spry 0.6.3's own window in Saver mode, shown with example apps (the names and numbers in the
pictures are examples, not a measurement). **How** says what Spry is doing to a process (eased,
pulsed, frozen, or at full speed while the app in front rests; explained under
[How it works](#how-it-works)); **For** says how long. The window follows your system's light or dark
setting until you choose one at the bottom left, and fits a small window:</sub>

<p><img src="docs/dashboard-dark.png" width="66%" alt="The same page in dark mode">&nbsp;<img src="docs/dashboard-phone.png" width="24%" alt="The same page in a narrow window"></p>

**Overview** plots the last minute of whole-computer CPU usage, with a live **ON**, **OFF** or **PAUSED** label attached to the newest point. The strip inside the chart marks when each state was observed; gaps mean no readings.
Choose **Record OFF reference** to release holds for 65 seconds (first five seconds discarded).
Spry resumes its configured behavior without changing your settings, even with the window closed. The dashed
line overlays that earlier OFF minute on the live timeline; the difference is current CPU minus the recorded
OFF average, in percentage points. Refresh the reference under a similar workload. It is not a simultaneous
measurement or an estimate of CPU saved.
For a controlled comparison, open **Measure → Run comparison**. Each chart uses the same zero baseline
for Off and On; throughput is better when higher, while typical and p95 response times are better when lower.
The latest result is saved on this computer. A live change in CPU use alone is not proof of a speedup.

## On an Android phone

- **Android:** [`Spry-android.apk`](https://github.com/juw12/spry/releases/download/android/Spry-android.apk): open the file on the phone and allow your browser to install apps once.
- **iPhone:** open https://juw12.github.io/spry/ in Safari, tap Share, then **Add to Home Screen**.

It opens the Spry guide (https://juw12.github.io/spry/: this README laid out for a phone) in a full-screen window of its own ([`android/`](android/README.md)) and changes with it. Spry itself runs on a computer and is installed the way this README says; the app does not contain it. The build is opened on an Android emulator on GitHub before it is published: it installs, opens and stays up. No real phone has run it yet.

## What you get

Measured by the program itself (`spry bench`) on GitHub's test machines, which have 3 or 4 processor
threads like many older laptops. The test starts twice as many busy background processes as the
machine has threads, then measures how much work the app in front gets done and how fast it reacts
to a click. On Windows and Linux the test is repeated with only 2 threads allowed, as a stand-in for
a smaller machine.

These tables measure the time while the app in front is working, with the hold Performance mode
uses under this load. Saver mode holds at least as much: since 0.6.4 it also keeps the held apps to
about a quarter of the processor threads on Windows and Linux. What the app in front gains with
that narrower hold has not been measured separately. The modes differ most while the app in front
rests: Performance lends the processor back (measured under
[What it costs the apps that are held](#what-it-costs-the-apps-that-are-held)), Saver keeps
holding (measured under [Saver and Performance](#saver-and-performance)).

<!-- bench:start -->
| System | Runs | Threads | Kept free | Quiet | Busy, Spry off | Busy, Spry on | Gain (lowest-highest) | Of quiet speed | With Strong mode | Slowest 5% of reactions, off -> on |
|---|---|---|---|---|---|---|---|---|---|---|
| Windows, x64 | 5 | 4 | 2 | 252 | 189 | 251 | **1.32x** (1.29-1.37) | 100% | not offered | 8.4 -> 4.2 ms |
| macOS 15, Apple silicon | 5 | 3 | none | 243 | 106 | 237 | **2.17x** (1.91-4.06) | 93% | 2.37x | 5.1 -> 5.9 ms |
| macOS 15, Intel | 5 | 4 | none | 249 | 93 | 241 | **2.48x** (1.99-3.03) | 99% | 2.68x | 4.4 -> 4.4 ms |
| Linux (Ubuntu), x64 | 5 | 4 | 2 | 249 | 113 | 250 | **2.20x** (2.02-2.85) | 100% | 2.20x | 12.1 -> 4.2 ms |
| Windows, x64, on 2 threads only | 5 | 2 | 1 | 251 | 76 | 241 | **3.03x** (2.74-3.89) | 96% | not offered | 4.3 -> 4.1 ms |
| Linux (Ubuntu), x64, on 2 threads only | 5 | 2 | 1 | 250 | 89 | 229 | **2.60x** (2.52-2.61) | 91% | 2.63x | 14.1 -> 4.3 ms |
| Linux (Ubuntu), ARM64 | 5 | 4 | 1 | 251 | 104 | 249 | **2.39x** (1.84-2.89) | 99% | 2.40x | 12.0 -> 4.1 ms |
| Linux (Ubuntu), ARM64, on 2 threads only | 5 | 2 | 1 | 251 | 84 | 251 | **3.02x** (2.03-3.10) | 100% | 3.02x | 11.9 -> 4.0 ms |

Each row is the median over its CI runs ([44508838506](https://github.com/juw12/spry/actions/runs/44508838506), [44508849036](https://github.com/juw12/spry/actions/runs/44508849036), [44508870158](https://github.com/juw12/spry/actions/runs/44508870158), [44508880495](https://github.com/juw12/spry/actions/runs/44508880495), [44510374146](https://github.com/juw12/spry/actions/runs/44510374146)). Work is in steps per second.
<!-- bench:end -->

These runs were made with the 0.3.1 code. The gain is measured against the same plain busy
processes as in every earlier version, and with Strong mode off, which is how Spry now comes on
every system. The 0.6.4 code was measured again for the Saver table further down; what the same
test gave the app in front in those runs is shown there, in its last column.

**The weak results, said plainly:**

- On the Windows test machine with all 4 threads the gain is still the smallest (1.32x), because
  Windows already gives the app in front most of what it needs. With Spry it reaches 100% of a quiet
  machine. The gain there also depends on which processor the test machine happens to have: when
  the released 0.2.1 and 0.3.0 were measured in turns on the same six machines
  ([run 44504694485](https://github.com/juw12/spry/actions/runs/44504694485), values in
  `docs/bench/ab-0.2.1/`), both gave 1.26-1.30x on one processor model and 1.33-1.47x on the three
  others, and on the same machine 0.3.0 was never more than 0.03 below 0.2.1. The five runs of
  0.3.0 gave 1.38x here and these five give 1.32x, with nothing changed in between for the time
  the app in front works.
- On macOS a normal program cannot keep part of the processor free, and how apps are held while
  the app in front works has not changed since 0.1.1. The two macOS test machines vary most
  between runs, so read their rows as "about twice", not to the decimal: Apple silicon gave 1.91x
  to 4.06x in these five runs and reaches 93% of a quiet machine. Intel gives 2.48x (99% of quiet)
  now, where the five runs of 0.3.0 gave 2.12x (90%) and those of 0.2.0 gave 2.28x, with the same
  code for this part each time. Reactions on the two macOS test machines were already short
  (4 to 5 ms) and did not get shorter; on Apple silicon they were a little longer with Spry on
  (5.1 -> 5.9 ms).
- The app in front is furthest from a quiet machine on Linux limited to 2 threads (91%).
- After the app in front has rested, the hold is not back at once: 0.6 to 1.1 seconds on the test
  machines (see the next section). An app in front that keeps using more than 5% of one processor
  core while you only look at it (a video call, a page with moving pictures) never counts as
  resting, so nothing is lent then.
- The app in front in this test uses one processor thread. An app that uses all of them gains less:
  it has the kept-free threads to itself and shares the rest. That case is not measured yet.
- The "2 threads only" rows are the same test machines limited to two threads. Nobody has measured
  Spry on a real old laptop yet.

**From version to version**, same test machines, same load, median gain:

<!-- bench-change:start -->
| System | 0.1.1 | 0.2.0 | 0.3.1 (now) | With pulses, 0.2.0 | With pulses, now |
|---|---|---|---|---|---|
| Windows, x64 | 1.27x | 1.39x | 1.32x | - | - |
| macOS 15, Apple silicon | 2.21x | 2.13x | 2.17x | 2.17x | 2.37x |
| macOS 15, Intel | 2.10x | 2.28x | 2.48x | 2.50x | 2.68x |
| Linux (Ubuntu), x64 | 1.09x | 2.36x | 2.20x | 2.37x | 2.20x |
| Windows, x64, on 2 threads only | - | 2.99x | 3.03x | - | - |
| Linux (Ubuntu), x64, on 2 threads only | - | 2.53x | 2.60x | 2.56x | 2.63x |
| Linux (Ubuntu), ARM64 | - | 2.07x | 2.39x | 2.09x | 2.40x |
| Linux (Ubuntu), ARM64, on 2 threads only | - | 3.01x | 3.02x | 3.01x | 3.02x |
<!-- bench-change:end -->

The first three columns are easing only, with no process ever stopped; the last two add the pulses
of Strong mode. Version 0.1.1 had nothing on Linux that held the processor back without pausing
apps (1.09x), and reached 92% of a quiet machine on Windows. Versions 0.3.0 and 0.3.1 did not
change how apps are held while the app in front works, so where the "now" column differs from 0.2.0
(lower on Windows and Linux x64, higher on macOS Intel and Linux ARM64) that is the spread between
runs and test machines, not a change in Spry. The five runs of 0.3.0, with the same code for this
part, show how wide that spread is: 1.38x on Windows, 2.90x on Linux x64, 2.12x on macOS Intel and
2.78x on Linux ARM64. What 0.3 changed is in the next section. The reaction times of 0.1.1 (up to 126 ms on macOS) were
mostly the test's own timer waking late on these virtual machines, so they are not compared here;
since 0.2.0 every reaction is timed from a click sent by a second thread. The earlier results are
kept in `docs/bench/v0.1/`, `docs/bench/v0.2/` and `docs/bench/v0.3.0/`.

- **Quiet** = nothing else running. **Busy, Spry off** = background processes take what they want.
  **Busy, Spry on** = the same load, held back by Spry as it comes, with no setting changed.
- **With Strong mode**: the gain with pulses added (macOS and Linux). Since 0.3.0 Strong mode is off
  everywhere until you tick it, also on Linux: there the pulses added almost nothing for the app in
  front and cost the held apps a large part of their speed (see
  [What it costs the apps that are held](#what-it-costs-the-apps-that-are-held)). If you used an
  earlier version, your saved settings are kept, so Strong mode stays on until you untick it.
- **Kept free**: how many processor threads background apps stay off while the machine is under
  pressure (Windows and Linux; macOS gives a normal program no way to do this). In Saver mode they
  stay off them all the time, and since 0.6.4 they keep to about a quarter of the threads.
- **Slowest 5% of reactions**: a second thread "clicks" 60 times a second; this is how long the app
  in front takes to answer, for the slowest twentieth of the clicks.
- These are shared virtual machines and the numbers move between runs, which is why the lowest and
  highest gain are shown. Your computer will differ, so measure it: press **Measure now** on the page
  (under a minute), or run `spry bench`.

**What Spry itself costs.** Measured half a minute after a fresh install on the same test machines
([install check of 0.2.0, run 44476935590](https://github.com/juw12/spry/actions/runs/44476935590)):

| System | Memory | Processor once settled | Processor time in the first 35-45 s, start included |
|---|---|---|---|
| Windows | 12.5 MB | 0.1% of one core | 0.14 s |
| macOS, Apple silicon | 30.3 MB | 1.4% of one core | 0.55 s |
| macOS, Intel | 21.1 MB | 1.2% of one core | 0.96 s |
| Linux, x64 | 1.3 MB | 0.1% of one core | under 0.01 s |
| Linux, ARM64 | 1.3 MB | 0.1% of one core | under 0.01 s |

On a busy machine it looks more often. One reading taken by hand, not from a linked run: on the
author's Windows PC (8 cores, 370 processes, 75% busy) it used 7.7 MB and about 2% of one core.

## What it costs the apps that are held

Holding an app back means it gets less done. Up to 0.2 that was true all the time: a held app
stayed held while you were reading or away, when nothing needed the processor. Since 0.3.0 the time
can be lent: in Performance mode, while the app in front rests, the held apps run as if Spry were
not there. Saver mode, the default since 0.5, keeps them held on purpose: that is where its saving
comes from. Up to 0.6.3 its price was the "still held" column below. Since 0.6.4 Saver holds
harder on Windows and Linux, and its price is in the tables under
[Saver and Performance](#saver-and-performance).

Measured in the same runs as the table above, with a second set of busy background processes that
count their own work. 100% is what they get done with Spry off.

<!-- bench-cost:start -->
| System | Front app works: held (lowest-highest) | Front app rests: still held (0.2; Saver mode up to 0.6.3) | The same with pulses (0.2 on Linux) | Front app rests: lent (Performance mode, lowest-highest) | Hold back after (slowest) |
|---|---|---|---|---|---|
| Windows, x64 | 56% (55-57) | 50% | not offered | **100%** (99-100) | 1.1 s (1.1 s) |
| macOS 15, Apple silicon | 54% (17-86) | 55% | 30% | **99%** (97-102) | 0.8 s (1.1 s) |
| macOS 15, Intel | 95% (64-115) | 93% | 32% | **100%** (82-104) | 0.6 s (0.9 s) |
| Linux (Ubuntu), x64 | 54% (54-55) | 50% | 20% | **100%** (99-101) | 0.8 s (0.9 s) |
| Windows, x64, on 2 threads only | 49% (37-62) | 48% | not offered | **100%** (100-101) | timed on all threads only |
| Linux (Ubuntu), x64, on 2 threads only | 56% (55-61) | 50% | 20% | **100%** (100-100) | timed on all threads only |
| Linux (Ubuntu), ARM64 | 80% (80-82) | 75% | 30% | **100%** (100-100) | 1.0 s (1.0 s) |
| Linux (Ubuntu), ARM64, on 2 threads only | 57% (57-60) | 50% | 20% | **100%** (100-100) | timed on all threads only |
<!-- bench-cost:end -->

- **Front app works: held** is what the held apps still get done while the app in front is busy.
  This part is the price of the gain and has not changed: the processor is finite, and while you
  work the others wait.
- **Front app rests: still held** is what they got up to 0.2 while the app in front did nothing,
  and what they got in Saver mode up to 0.6.3 (0.6.4 is measured under
  [Saver and Performance](#saver-and-performance)). **The same with pulses** is what they got on
  Linux as 0.2 came, or anywhere with Strong mode ticked. Up to 0.2 nobody had chosen to lose this
  time; in Saver mode it is the saving you asked for.
- **Front app rests: lent** is what they get in Performance mode.
- **Hold back after** is how long after the app in front starts working again the others are held
  again, timed in a test that runs the whole program (median, and the slowest of the five runs).
  During that moment the app in front shares the processor as if Spry were off.

Said plainly: on the macOS Apple silicon test machine the "front app works" figure went from 17% to
86% between runs (3% to 76% in the runs of 0.3.0), so that row says little; the system's background
state there can leave a held app almost nothing. On macOS Intel two of the five runs showed the
lent apps at 82% and 90% of their free speed; every other reading in that column is 97% or more.
And a rest only
counts after 6 seconds below 5% of one processor core, so short pauses between keystrokes lend
nothing.

**On a real, busy computer the test machines were wrong about Windows.** The test machines run
nothing but the test. The author's Windows PC (8 threads) was busy with his own background jobs,
which run at a lower priority than normal. There 0.3.0, released an hour before, lent far less
than the table said: it gave the processor threads back but left the lent apps at lower priority,
and they had to share with those other jobs. 0.3.1 gives the priority back as well. Both released
downloads, run in turns on that PC (`spry bench`, readings taken by hand, kept in
`docs/bench/by-hand/`):

| On that PC, while the app in front rests | Held apps get, of their speed with Spry off |
|---|---|
| Lent by 0.3.0 (four readings) | 38%, 41%, 48%, 51% |
| Lent by 0.3.1 (two readings) | **99%, 106%** |
| Not lent, held the way 0.2 did (all six readings) | 0% to 2% |

The same readings show the price while the app in front works more sharply than the test machines
do: the held apps got 0% to 2.5% of their free speed there, not about half. One reason is the test
itself: it holds only its own background processes, so the author's other jobs, which were not
held, went ahead of them. With Spry running for real those jobs would be held as well and all of
them would share what the app in front leaves. That case has not been measured. The app in front
gained 1.96x to 2.21x on that PC (90% to 99% of a quiet machine) in the same six readings.

### Saver and Performance

**Settings → What Spry is for** chooses the mode. Until 0.4 Spry only had what is now Performance
mode; since 0.5 a new install starts in Saver mode.

In **Saver** mode nothing is lent: a busy background app stays held until you switch back to it.
It is held the deeper way all the time (on Windows also Efficiency mode, low memory priority and
part of the processor kept free; on Linux part of the processor kept free; on macOS the background
state, which is already the deep one). Since 0.6.4, on Windows and Linux, the held apps together
also keep to about a quarter of the processor threads, on the weakest cores: one of four, two of
eight, and one of two or three. That is what lets Saver use less when the computer still has
processor to spare, and it is why held work takes longer than in Performance mode. macOS gives a
normal program no way to do this. Spry itself also looks less often: every four seconds while
its window is open, every eight on a calm machine. The app in front, the apps you set to "never
slow" and the built-in "never touched" list stay exempt. "Lend idle time" is kept for when you
switch to Performance.

What Saver mode saves is processor work, and with it heat and fan noise. It is not a wattage cap,
and battery life has not been measured. It does not shrink the memory of other apps: on macOS and
Linux the system decides about memory; on Windows held apps get a low memory priority.

**Measured on the test machines**, Spry 0.6.4, by `spry bench`: the app in front rests and the
background is held the way Saver mode holds it. The same machines and the same test as under
[What you get](#what-you-get), and once more with two busy background processes only, because a
computer is not always overloaded.

<!-- bench-saver:start -->
**More busy processes than processor threads** (the load of the tables above):

| System | Runs | Threads | Busy processes | Threads the held apps run on | Work the held apps get done, Spry off = 100% (lowest-highest) | Whole computer busy, off -> Saver (lowest-highest) | The same two on 0.6.3 | Gain for the app in front in the same runs (lowest-highest) |
|---|---|---|---|---|---|---|---|---|
| Windows, x64 | 6 | 4 | 8 | 1 of 4 | **25%** (24-27) | 100% -> **28%** (26-31) | 49%, 52% busy | 1.38x (1.30-1.42) |
| macOS 15, Apple silicon | 6 | 3 | 6 | all | **48%** (8-87) | 100% -> **100%** (100-100) | 70%, 100% busy | 2.40x (1.93-2.65) |
| macOS 15, Intel | 6 | 4 | 8 | all | **94%** (75-105) | 100% -> **100%** (100-100) | 93%, 100% busy | 2.92x (1.87-4.90) |
| Linux (Ubuntu), x64 | 6 | 4 | 8 | 1 of 4 | **26%** (25-27) | 100% -> **25%** (25-25) | 50%, 50% busy | 2.24x (1.98-2.56) |
| Windows, x64, on 2 threads only | 6 | 2 | 4 | 1 of 2 | **50%** (50-56) | 50% -> **26%** (25-26) | 50%, 26% busy | 2.98x (2.97-3.81) |
| Linux (Ubuntu), x64, on 2 threads only | 6 | 2 | 4 | 1 of 2 | **50%** (50-53) | 50% -> **25%** (25-25) | 50%, 25% busy | 2.49x (2.42-2.58) |
| Linux (Ubuntu), ARM64 | 6 | 4 | 8 | 1 of 4 | **25%** (25-25) | 100% -> **25%** (25-25) | 75%, 75% busy | 2.69x (2.08-2.99) |
| Linux (Ubuntu), ARM64, on 2 threads only | 6 | 2 | 4 | 1 of 2 | **50%** (50-50) | 50% -> **25%** (25-25) | 50%, 25% busy | 3.00x (2.03-3.01) |

**Two busy processes only**, so with processor to spare:

| System | Runs | Threads | Busy processes | Threads the held apps run on | Work the held apps get done, Spry off = 100% (lowest-highest) | Whole computer busy, off -> Saver (lowest-highest) | The same two on 0.6.3 | Gain for the app in front in the same runs (lowest-highest) |
|---|---|---|---|---|---|---|---|---|
| Windows, x64 | 6 | 4 | 2 | 1 of 4 | **50%** (50-50) | 50% -> **25%** (25-25) | 98%, 50% busy | 1.08x (1.05-1.11) |
| macOS 15, Apple silicon | 6 | 3 | 2 | all | **73%** (4-86) | 86% -> **82%** (75-100) | 82%, 79% busy | 1.00x (0.91-1.03) |
| macOS 15, Intel | 6 | 4 | 2 | all | **92%** (70-96) | 53% -> **53%** (52-100) | 97%, 53% busy | 1.02x (0.92-1.46) |
| Linux (Ubuntu), x64 | 6 | 4 | 2 | 1 of 4 | **50%** (50-50) | 50% -> **25%** (25-25) | 96%, 50% busy | 1.09x (1.04-1.10) |
| Linux (Ubuntu), ARM64 | 6 | 4 | 2 | 1 of 4 | **50%** (50-50) | 50% -> **25%** (25-25) | 100%, 50% busy | 1.00x (1.00-1.00) |

Each row is the median over its CI runs of the 0.6.4 code ([44668866888](https://github.com/juw12/spry/actions/runs/44668866888), [44669537954](https://github.com/juw12/spry/actions/runs/44669537954), [44669545117](https://github.com/juw12/spry/actions/runs/44669545117), [44669552273](https://github.com/juw12/spry/actions/runs/44669552273), [44669559397](https://github.com/juw12/spry/actions/runs/44669559397), [44669566421](https://github.com/juw12/spry/actions/runs/44669566421)): 6 runs per row. The 0.6.3 column is the median over the runs of the 0.6.3 code ([44660688597](https://github.com/juw12/spry/actions/runs/44660688597), [44660697840](https://github.com/juw12/spry/actions/runs/44660697840), [44660706129](https://github.com/juw12/spry/actions/runs/44660706129), [44660714265](https://github.com/juw12/spry/actions/runs/44660714265), [44660722516](https://github.com/juw12/spry/actions/runs/44660722516), [44661399638](https://github.com/juw12/spry/actions/runs/44661399638), [44661407887](https://github.com/juw12/spry/actions/runs/44661407887), [44661417018](https://github.com/juw12/spry/actions/runs/44661417018), [44661425398](https://github.com/juw12/spry/actions/runs/44661425398), [44661433237](https://github.com/juw12/spry/actions/runs/44661433237), [44662489281](https://github.com/juw12/spry/actions/runs/44662489281)): 11 runs per row in the first table, 6 in the second.
<!-- bench-saver:end -->

- **Work the held apps get done** is the price: what the busy background processes still finish,
  with Spry off as 100%.
- **Whole computer busy** is the saving: the share of all processor threads in use, read from the
  system, with Spry off and in Saver mode. On the "2 threads only" rows the test is confined to
  two of the machine's four threads and the reading is over all four, so 50% there is two full
  threads.
- **Threads the held apps run on** is how many processor threads the held apps share in Saver
  mode. "all" on macOS, where a normal program cannot set this.
- **The same two on 0.6.3** is the work the held apps got done and how busy the computer was in
  Saver mode with the 0.6.3 code, which only kept held apps off the kept-free threads.
- **Gain for the app in front** is the test of [What you get](#what-you-get) in these runs.

**What these numbers say, plainly:**

- On Windows and Linux, Saver mode now lowers processor use also when the computer has processor
  to spare. With two busy processes on four threads the computer went from 50% busy to 25% on all
  three machines; with 0.6.3 it stayed at 50%. The price: the two held apps share one thread and
  got 50% of their work done, where 0.6.3 left them 96% to 100%.
- Under the full load the computer went from 100% busy to 25% on Linux and 28% on Windows. With
  0.6.3 it went to 50%, 52% and 75% (ARM64). The held apps got about a quarter of their work done;
  with 0.6.3 it was about half, and three quarters on ARM64.
- On the "2 threads only" rows nothing changed against 0.6.3. One of the two threads was already
  kept free, so the held apps were on one thread before as well.
- So the price is real. In these tests a busy job in the background took two to four times as long
  in Saver mode on Windows and Linux. If you are waiting for that job, bring its window to the
  front, set the app to "never slow", or choose Performance mode.
- On the two macOS test machines 0.6.4 changes nothing, because macOS gives a normal program a
  lower priority for another app, not a cap. Under the full load the computer stayed 100% busy and
  the held apps got 48% (Apple silicon, 8% to 87% between runs) and 94% (Intel) of their work
  done. With two busy processes it went from 86% to 82% and from 53% to 53%. On the real MacBook
  below the same hold did much more (72% busy to 39%) than on these virtual Macs. Which of the two
  your Mac resembles is not known until you measure it.
- The macOS Intel test machine was itself slow in five of the six full-load runs: with nothing
  else running it did 35 to 116 steps per second, where it usually does about 250. Its rows are
  kept in the table and say little about Spry.
- Efficiency mode on Windows can also lower the processor's clock speed on a real laptop. A
  virtual test machine cannot show that, and it has not been measured.
- So "uses less processor" is measured on Windows and Linux test machines under both loads, and on
  one real Mac with 0.5.0. The test machines are virtual and have four threads. A real laptop with
  0.6.4 in Saver mode has not been measured. Press **Measure now** or run `spry bench` to see what
  your computer does.

**On a real, busy Windows PC.** The author's PC (8 threads, busy with his own jobs), the released
0.6.3 and 0.6.4, three readings of `spry bench` each, taken by hand and kept in
`docs/bench/by-hand/`:

| Version | Load of the test | Held apps got, of their speed with Spry off | Whole PC busy, off -> hold kept | Gain for the app in front |
|---|---|---|---|---|
| 0.6.3 | 16 busy processes | 7% | 100% -> 93% | 2.56x |
| 0.6.3 | 16 busy processes, again | 16% | 100% -> 88% | 2.90x |
| 0.6.3 | 2 busy processes | 86% | 62% -> 52% | 1.12x |
| 0.6.4 | 16 busy processes | 29% | 100% -> 77% | 2.77x |
| 0.6.4 | 16 busy processes, again | 22% | 100% -> 78% | 2.92x |
| 0.6.4 | 2 busy processes | 58% | 81% -> 77% | 0.88x |

The test holds only its own processes. The author's other jobs were not held, so they took most of
what the held ones gave up, and the PC stayed mostly full. With Spry in Saver mode for real those
jobs would be held as well; that has not been measured. With 0.6.4 the held processes were kept to
2 of the 8 threads and the PC went from full to about 77% busy; before the 0.6.4 readings the
PC was already 73 to 76% busy with the author's jobs. In both rows with 2 busy processes the
author's jobs moved during the reading, so those points are not a saving to count on, and in the
0.6.4 one the app in front did less work with Spry on than with it off (0.88x). That reading
cannot say why.

**Measured on a real laptop**, Spry 0.5.0 on a MacBook Pro 2019 (Intel i5-8257U, 8 threads, macOS 15.7), 19 Nov 2026.
Four busy processes in the background that belong to no app, the app in front resting, 30 seconds
per state after 16 seconds to settle. The processor time is what the system accounted to the four.

| State | Processor time the four busy processes got | Whole computer |
|---|---|---|
| Off | 94% of a core each | 72% busy |
| On, Performance | 94% of a core each (lent) | 73% busy |
| On, Saver | 29% of a core each (held) | 39% busy |
| Off again | 93% of a core each | 72% busy |

In Saver mode the busy background apps got 70% less processor time and the computer was at 39%
instead of 72%. They also get that much less done. Performance mode, with the app in front resting,
is the same as off, as it is meant to be. One computer, one run per state: measure yours under
**Measure**, where the fourth chart shows the same thing. Other systems hold differently (see the
table under [How it works](#how-it-works)), so the saving there will differ.

Pausing a process did not give memory back on that Mac: a paused process that held 600 MB still
held 604 MB a minute later (43% of memory was free). That is why Saver mode does not pause apps by
itself; "Freeze in background" stays a rule you set per app.

## The load limit

New in 0.7.0, on unless you switch it off (Settings, **Load limit**).

Why it is there: a laptop ran several AI assistants for 100 hours at about 80% of its processor.
At no moment was it under pressure for long, so nothing ever held anything back, and it never
rested either. Then its drive dropped out in the middle of the work. A moment at 100% does a
computer no harm. Days at 80% are what make the heat.

So Spry keeps the processor's average use, minute by minute, over the last hour. When that average
reaches the limit (75% unless you change it), the computer is over its limit, and until it has
rested Spry does this, whichever mode it is in:

- nothing is lent: busy apps in the background stay held, also while the app in front rests;
- they are held the deeper way and, on Windows and Linux, kept to about a quarter of the processor
  threads, as in Saver mode;
- those that use half a processor core or more are also paused in short pulses (120 ms paused,
  80 ms running). A lower priority alone takes no work away while nothing else wants the
  processor; a pulse does;
- with the screen locked or nobody at the computer, where no app is in front, busy apps are held
  as well. That is when nobody is there to notice how hard it works. Without an app in front they
  are only eased, never paused.

It counts as rested once the average is 5 points under the limit (70% for a limit of 75%). The
page says **Over its load limit** while it lasts, and Settings shows the average it is judged by.
The limit (30 to 100%) and the stretch (10 to 240 minutes) can be changed there, or in
`config.json` as `limit`, `limit_cpu` and `limit_minutes`.

What it does not do, so nobody counts on it:

- Spry reads no temperature. It limits the work that makes the heat.
- The app in front and everything it started is never held. If that app is what keeps the
  processor busy (a game, a render you are watching), the computer stays over its limit and only
  the background is held.
- "Never slow" rules still win, and system programs are never touched, as everywhere in Spry.
- It is not judged before 45 of the 60 minutes have been seen, and a computer that slept or was
  off starts fresh.
- What the hold saves depends on the system and the load. On the office PC it was written for,
  Saver's hold alone took 16 busy processes from 100% of the processor to about 77% (see
  [Saver and Performance](#saver-and-performance)); the pulses come on top. How far the limit
  brings a computer down, and how much cooler that leaves it, has not been measured yet.

The rule itself is tested on every system by CI (`limit::tests` and the `over_its_limit_*` tests
of the policy): an hour at 80% is over, a busy quarter of an hour is not, and the holds are the
ones listed above. It has not run for an hour on a real computer yet.

## What it cannot do

- It does not add memory or make the processor faster. It decides who goes first. Work in the
  background takes longer: in Performance mode while the app in front is working, in Saver mode
  for as long as it is in the background. How much longer is measured under
  [What it costs the apps that are held](#what-it-costs-the-apps-that-are-held).
- If the app in front is slow by itself with nothing else running, Spry changes nothing.
- A computer that is out of memory stays out of memory. On Windows, Spry can ask large background
  apps to give back memory they are not using; on macOS and Linux the system does not offer that.
- While the app in front works and the machine is under pressure, background apps on Windows and
  Linux run on fewer processor threads (on a 4-thread machine: 2), so they take longer. That is the
  price of the gain above. In Performance mode they get every thread back when the app in front
  rests. In Saver mode they stay on about a quarter of the threads all the time (on a 4-thread
  machine: 1), which in the tests cost them half to three quarters of their speed.
- The downloads are not signed yet, so Windows and macOS warn on first start (see Install). They
  can be checked another way: see "Check a download" under Install. What signing needs, and what
  is already prepared for it, is in [docs/signing.md](docs/signing.md).

## Install

**Windows 10 or 11**

1. Download [spry-windows-x64.zip](https://github.com/juw12/spry/releases/latest/download/spry-windows-x64.zip).
2. Open the zip and drag `spry.exe` to a folder you keep, for example Documents.
3. Double-click `spry.exe`. Windows shows "Windows protected your PC" because the file is not signed
   yet: choose **More info**, then **Run anyway**.

**macOS 11 or newer** (Apple silicon and Intel), without Terminal:

1. Download [Spry-macos.zip](https://github.com/juw12/spry/releases/latest/download/Spry-macos.zip).
2. Double-click the zip in Downloads, then drag **Spry** into the Applications folder.
3. Open Spry. macOS says "Spry" was not opened, because Spry is not signed by Apple yet: choose
   **Done** (not "Move to Trash", which some Macs call "Move to Bin").
4. Open System Settings > Privacy & Security, scroll down to Security and press **Open Anyway**
   next to Spry. macOS asks once more: press **Open Anyway** again, then type the name and
   password of an administrator of the Mac (on your own Mac that is you). Then Spry opens. The
   button is there for about an hour after step 3. (On macOS 14 or older: hold Control, click
   Spry, choose **Open**.) Apple describes the same steps in
   [Open a Mac app from an unknown developer](https://support.apple.com/guide/mac-help/mh40616/mac).

From then on Spry opens like any app and keeps itself up to date. Said plainly: on 20 Nov 2026 a
script went through these steps on a macOS 15 test machine with the released 0.6.3 and 0.6.4
downloads. Step 3's message, the **Open Anyway** button in Settings, the second question and the
password window all came up as written ([pictures](docs/mac-install/)). macOS did not let the
script press OK in the password window, so the last moment, Spry opening after the password, has
not been seen, and no person has clicked through with this version. After every release a test
machine also checks that the zip opens the way Finder opens it and that the app in it is whole and
is the released version.

**macOS with one line.** Open Terminal, paste this line, press Return:

```sh
curl -fsSL https://raw.githubusercontent.com/juw12/spry/main/install.sh | sh
```

It puts **Spry.app** into Applications (your own Applications folder when the shared one is not
yours to write to), starts it, and makes the `spry` command point into the app. macOS does not ask
this way, because the download did not come through a browser. This way is run on two Mac test
machines after every release.

**Linux** (x64 and ARM64): the same line as on a Mac. Spry has to see which app is in front:

| Desktop | Does Spry see the app in front? |
|---|---|
| Any desktop on X11 | Yes. |
| Wayland with sway | Yes, tested on every change. |
| Wayland with Hyprland or niri | Written from their documentation, not yet tried on a real session. `spry front` tells you. |
| Wayland with GNOME or KDE (the default on current Ubuntu and Fedora) | No: these desktops do not tell other programs. Spry then only follows the rules you set on its page, so it helps less there. |

**After the first start** Spry opens its window and works at once, with no settings. On Windows and
macOS that is a window of its own, like any app (it uses the web view that comes with the system:
WebView2 on Windows, WebKit on macOS; on a Windows without WebView2 the page opens in the browser).
Closing the window does not stop Spry: it goes on in the tray (Windows) or the menu bar (macOS), and
the icon opens the window again. The window is a process of its own, so a closed window costs
nothing: Spry itself stays as small as the page says. On Linux the page opens in your browser;
run `spry open` for it.
Tick **Start Spry when I sign in** under Settings to keep it. The page lists the apps that are busy,
held back or have a rule; apps that are doing nothing are folded away behind one button.

**To undo everything:** press **Quit Spry** under Settings (every app is put back at that moment), untick
"Start Spry when I sign in", and delete the file (on a Mac: Spry.app in Applications, and the `spry`
link in `~/.local/bin`). Spry leaves nothing else behind except a small settings folder.

People who prefer a terminal on Windows: `irm https://raw.githubusercontent.com/juw12/spry/main/install.ps1 | iex`

**Check a download.** Every file on the [release page](https://github.com/juw12/spry/releases/latest)
has its SHA-256 checksum next to it (`<file>.sha256`). The install lines above compare the two and
install nothing when they differ. Each file also has a build record, signed by GitHub, that says
which source commit and which workflow produced it. With the
[GitHub command line tool](https://cli.github.com/):

```sh
gh attestation verify spry-linux-x64.tar.gz -R juw12/spry
gh attestation verify Spry-macos.zip -R juw12/spry
```

This shows the file was built from this repository's source by its release workflow, and not
changed afterwards. It is not the same as a signature from Microsoft's or Apple's programmes, so the
first-start warnings on Windows and macOS remain.

## Updates

Spry keeps itself up to date. Half a minute after it starts, and then once a day, it asks GitHub
which release is the newest (in an hour again when it got no answer, as on a laptop that was not
online yet). When that one is newer than the Spry that is running, Spry:

1. downloads the file for your system from the release page into a folder next to itself;
2. compares it with the SHA-256 checksum published next to it;
3. starts the downloaded program once and checks that it answers with the announced version;
4. puts it in the place of the one that is there (on a Mac: the whole Spry.app);
5. quits the usual way, so every app is put back, and starts again as the new version. If its
   window was open, it opens again.

When a step fails nothing is replaced: the Spry that is there keeps running, and its page says what
went wrong, with a button to try again.

- **To decide yourself**, untick **Update Spry automatically** under Settings. Spry then only says
  on its page that a newer version is out, with an **Update now** button.
- **From a terminal:** `spry update` does it now.
- **Never look:** start Spry with the environment variable `SPRY_UPDATE_FROM` set to nothing. Set to
  an address, Spry looks there instead of GitHub (a folder with `latest.json` and the downloads).

What you should know about it:

- The daily look is one request to `github.com`: the address of the newest release's page. Only
  when that gives no answer Spry asks `api.github.com`, which answers 60 times an hour per network
  address, shared by everyone behind the same office or provider. Like every request it shows
  GitHub your internet address. Nothing about you, your computer or your apps is in it, and Spry
  makes no other connection.
- Behind a proxy, `curl` uses the one named in the environment variable `HTTPS_PROXY`. Tried on an
  office PC that reaches GitHub only that way.
- The checksum shows that the download arrived whole and is the file on the release page. It is
  the same check the install lines make. Spry does not check the build record (see "Check a
  download" above) by itself, so it trusts the release page of this repository.
- It uses the `curl` and `tar` that come with the system (on Windows: since Windows 10 of 2018).
  Where they are missing, or where Spry may not write to its own folder, it only tells you.
- A Spry that updated itself restarts on its own, once per release. Apps that were held run at
  full speed for the second or two in between.
- Tested on every change on all five systems with a folder standing in for the release page, and
  after every release against the real release page (see [Safety](#safety)). Not yet tried: a Mac
  on which Spry was started by "Start Spry when I sign in".
- 0.6.0, the first version that updates itself, did not come back after an update when its window
  was open or had been open a moment before: the check against the real release page found that on
  Windows and macOS within minutes of the release, and 0.6.1 fixes it. If you have 0.6.0, start
  Spry again by hand after it has updated; from 0.6.1 on it comes back by itself.
- Until 0.6.1 the look went to `api.github.com` only, and a look that got no answer was repeated a
  day later. On a shared network address that answer can be "too many requests": it happened to
  the check of 0.6.1 on one of GitHub's Mac test machines. 0.6.1 also brought its window back when
  anything had asked it for its numbers in the last 10 seconds (`spry status` in a script, for
  example), not only when the window was open. 0.6.2 changes all three.

## How it works

Every half second Spry asks the system which app is in front. Every two seconds (every five while
the computer is calm) it measures all running apps.

- **The app in front is never touched**, together with everything it started and the app that
  started it.
- An app you just left keeps full speed for 10 seconds.
- A background app that uses more than 10% of one processor core is **eased**: it only gets the
  processor when the app in front does not need it.
- In Saver mode easing is always the deeper kind, nothing is lent, and on Windows and Linux held
  apps keep to about a quarter of the processor threads (see
  [Saver and Performance](#saver-and-performance)). The next two points describe Performance mode.
- When the whole processor is above 80% for 4 seconds, easing goes deeper. On Windows and Linux,
  eased apps then stay off part of the processor (about a quarter of the threads, in whole cores,
  never more than half), so the app in front always finds a free one. Strong mode (off until you
  tick it) also **pulses** the heaviest background processes (120 ms paused, 80 ms running).
- **The time is lent.** When the app in front and everything it started have used less than 5% of
  one processor core for 6 seconds, the held apps get the whole processor back and the page shows
  them at "Full speed". Spry then looks twice a second, and the hold returns as soon as the app in
  front works again. Switching to another app counts as working. Apps you set to "always slow" or
  "freeze" are not lent anything, and one tick on the page ("Lend idle time") switches this off.
- You can set a rule per app: never slow, always slow in the background, or **freeze** in the
  background (fully paused until you switch back to it).
- Calls, music, recording and the desktop are on a built-in "never touched" list, and every program
  that lives in a system folder is skipped whatever its name.

What "eased" means on each system:

| System | Eased | Notes |
|---|---|---|
| Windows 10/11 | Lower priority class. Under pressure also Efficiency mode, low memory priority, and part of the processor kept free. In Saver mode that is so all the time, on about a quarter of the processor threads. | Also releases unused memory of large background apps when memory runs low. An app that picked its own processor threads keeps them. |
| macOS 11+ | The system's background state (what `taskpolicy -b` sets): lowest processor priority, throttled disk. | Apple silicon and Intel. |
| Linux | Idle disk priority. Under pressure part of the processor is kept free. In Saver mode that is so all the time, on about a quarter of the processor threads. | A normal user cannot raise a priority again after lowering it, so Spry does not lower it; which threads an app may use can be put back, so Spry uses that, and gives every thread back exactly what it had. |

## Safety

- **Everything is put back** when you pause Spry, switch it off or quit it. Spry saves what each
  process looked like before, and restores exactly that.
- If Spry is killed or the computer crashes, the list of held processes is on disk: the next start,
  or `spry rescue`, puts them back. A frozen app is resumed the same way.
- If an app changes its own priority (browsers do this for their tabs) or picks its own processor
  threads while Spry holds it, Spry leaves that choice alone and does not undo it.
- The system is left alone twice over: Spry runs without administrator rights, so it cannot reach
  system services or other users' apps; and it skips every program that lives in a system folder
  (`C:\Windows`, `/System`, `/usr/libexec` and the like) as well as a built-in list of desktop,
  sound, call, music and recording apps. Apps that come with the system, such as Photos or Safari,
  count as apps.
- The page is served on `127.0.0.1` only and needs a secret token that is created on your computer.
- A newer Spry replaces the one that is there only when the download matches its published
  checksum and answers with the announced version. Otherwise nothing is changed (see
  [Updates](#updates)).
- Storage cleaning shows what it found and removes nothing until you choose. It only looks at
  caches, temporary files and the bin, never at documents, downloads or app data.

This is tested on every change, on Windows, macOS (Apple silicon and Intel) and Linux (x64 and
ARM64) ([CI](https://github.com/juw12/spry/actions/workflows/ci.yml)): a real busy process is eased,
paused, frozen, kept off part of the processor, and put back; Spry is killed and `spry rescue` puts
the process back; on Linux, a process that picks its own processor threads while held keeps them.
A second check ([install](https://github.com/juw12/spry/actions/workflows/install.yml)) runs the
install steps from this page on each system, confirms the installed Spry starts and answers, feeds
the installer a changed download and confirms it is refused, has the installed Spry update itself
from the release page and start again, and verifies the build record of every download.

Each promise above has a test with a name you can read, run on every change:

| Promise | Test |
|---|---|
| Everything is put back, exactly | `easing_shows_and_is_put_back_exactly`, `freezing_stops_the_process_and_thawing_starts_it_again`, `under_pressure_part_of_the_processor_is_kept_free_and_given_back`, `saver_mode_keeps_a_held_app_to_a_few_threads_and_gives_them_back` in [tests/roundtrip.rs](https://github.com/juw12/spry/blob/main/tests/roundtrip.rs) |
| Put back after Spry is killed | `eases_shows_restores_and_rescues` in [tests/end_to_end.rs](https://github.com/juw12/spry/blob/main/tests/end_to_end.rs) |
| An app's own choice is left alone | `a_priority_the_app_set_itself_is_never_undone`, `a_choice_the_app_made_itself_while_eased_is_left_alone` in [tests/roundtrip.rs](https://github.com/juw12/spry/blob/main/tests/roundtrip.rs) |
| Held apps get full speed while the app in front rests, and the hold is back within 3 seconds | `lends_while_the_front_app_rests_and_holds_again_when_it_works` in [tests/end_to_end.rs](https://github.com/juw12/spry/blob/main/tests/end_to_end.rs), `lending_gives_the_speed_back_and_the_hold_can_return`, `the_fresh_reading_sees_the_front_app_work_and_rest`, `the_main_reading_sees_a_process_stop_completely` in [tests/roundtrip.rs](https://github.com/juw12/spry/blob/main/tests/roundtrip.rs), `held_apps_are_lent_the_time_while_the_front_app_rests`, `nothing_is_lent_when_switched_off_by_rule_or_without_a_known_front_app` in [src/policy.rs](https://github.com/juw12/spry/blob/main/src/policy.rs) |
| The app in front is never touched | `busy_background_is_eased_and_the_front_app_is_not`, `focus_on_a_child_window_covers_the_whole_app` in [src/policy.rs](https://github.com/juw12/spry/blob/main/src/policy.rs) |
| System programs and the "never touched" list are skipped | `programs_that_belong_to_the_system_are_recognised_by_where_they_live` in [src/config.rs](https://github.com/juw12/spry/blob/main/src/config.rs), `never_slow_and_protected_are_left_alone` in [src/policy.rs](https://github.com/juw12/spry/blob/main/src/policy.rs) |
| The page answers only with the token | `eases_shows_restores_and_rescues` (asks without and with a wrong token) in [tests/end_to_end.rs](https://github.com/juw12/spry/blob/main/tests/end_to_end.rs) |
| An update replaces Spry only with a download that matches its checksum and is the announced version | `an_older_spry_replaces_itself_with_the_newer_download_and_starts_again`, `on_a_mac_the_whole_app_is_replaced`, `a_download_that_is_changed_or_not_the_announced_version_changes_nothing` in [tests/update.rs](https://github.com/juw12/spry/blob/main/tests/update.rs), `a_higher_version_is_newer_and_anything_odd_is_not` in [src/update.rs](https://github.com/juw12/spry/blob/main/src/update.rs) |
| Storage: looking removes nothing, and only caches, temporary files and the bin are looked at | `sweep_removes_only_old_files_and_keeps_the_root`, `only_folders_well_inside_home_or_temp_are_accepted`, `no_cleaning_folder_is_inside_your_own_files` in [src/clean.rs](https://github.com/juw12/spry/blob/main/src/clean.rs) |

What went wrong along the way, so you can judge the testing. The largest fault got past every
test: 0.3.0 was released lending less than it claimed on Windows. It gave the processor threads back
but kept the lower priority, which costs nothing on a test machine that runs only the test, and cost
half the speed on a real PC busy with other work (see
[What it costs the apps that are held](#what-it-costs-the-apps-that-are-held)). It was found by
running the released download on a real PC and fixed in 0.3.1 the same day; the test that a lent
process looks exactly as it did before Spry touched it now runs on Windows as well as macOS. The
five CI runs behind the table all passed. A sixth
([44508859482](https://github.com/juw12/spry/actions/runs/44508859482)) passed on four machines
and was cancelled while still waiting for a macOS Apple silicon machine, so no test failed in it
and it is not in the table. Getting to 0.3.0 took several corrections. A new test failed 2 of 13 times
on macOS Intel; it was a real fault, not a shaky test: on macOS a process that had stopped
completely kept showing its last reading, so Spry could go on thinking a resting app was working.
After the fix the test passed 56 of 56 times. A second new test failed on Windows because the first
reading of a process that has just appeared is always about 0 there; the test now skips it. The
first way of measuring what the hold costs made the busy processes count their work, which changed
the load itself; measuring the released 0.2.1 beside it on the same machines showed that, and the
gain is now measured against the untouched processes with a separate counting set for the cost.
The same comparison showed that the Windows gain depends on the test machine's processor model.
One check did not pass on the five runs of 0.3.0: the median gain of every row was to be at least
the lowest 0.2.0 run of that row, and "Linux, ARM64, on 2 threads only" gave 3.005x against 3.007x,
a row where the app in front already runs at 100% of a quiet machine. On the five runs of 0.3.1 the
same check passes, by as little (3.025x against 3.007x in that row, 1.32x against 1.30x on
Windows), so it says more about the spread of these machines than about Spry.

Two of the seven runs of 0.1.1 failed a test on macOS Intel (one test expected one held process where Spry rightly held two;
one measured a busy process while other tests ran beside it). Both were faults in the tests and are
fixed. The reaction-time column of 0.1.1 measured the test's own timer more than Spry (see above).
The install check also showed 0.1.0 easing two
macOS system helpers it did not know by name. That was put back as always, and 0.1.1 skips every
program in a system folder.

## Commands

```
spry              start Spry and open its window
spry open         open the window of the running Spry
spry status       show what is being held back right now
spry bench        measure the effect on this computer (under a minute)
spry clean        show what storage can be cleared; add --yes to clear it
spry install      start Spry when you sign in      (spry uninstall undoes it)
spry update       get the newest Spry now (Spry also looks once a day; see Updates)
spry rescue       put processes back after a crash
spry front        show the process id of the app in front (checks that Spry can see it)
```

`spry bench --threads 2` runs the measurement on two processor threads only (Windows and Linux).
Since 0.6.3 `spry bench` also reports Saver mode: what the held apps get done while the app in
front rests, and how busy the whole computer is then, with Spry off and with the hold kept.
Since 0.6.4 it also prints how many processor threads the held apps keep to in Saver mode.

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
