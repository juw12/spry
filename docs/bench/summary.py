"""Builds the measurement tables in README.md from the CI results kept in docs/bench/.

Each folder docs/bench/run-<id>/ holds what `spry bench --secs 4 --json` printed on the
GitHub-hosted runners in CI run <id> (and again with `--threads 2` where the system allows it),
and <system>-hold-back-ms.txt: how long the whole-program test waited for the hold to come back
after the app in front started to work. The tables show the median over all kept runs and the
lowest and highest value seen, so nothing in the README is typed by hand.

Older versions are kept apart and never mixed in: v0.1/ (0.1.1: no kept-free processor threads,
reactions timed from a timer), v0.2/ (0.2.0: apps were held whatever the app in front did, and
pulses were the default on Linux) and v0.3.0/ (0.3.0: on Windows a process that was lent the
time kept its lower priority).

v0.6/ holds runs of the 0.6.3 code. From that version on the bench also reads how busy the whole
computer is while the app in front rests, with Spry off and with the background held the way
Saver mode holds it. In 0.6.3 Saver mode held apps the way the app in front needs it: on the
threads that are not kept free.

v0.6.4/ holds runs of the 0.6.4 code. From that version on Saver mode keeps held apps to about a
quarter of the processor threads on Windows and Linux. Those runs make the Saver table and
nothing else, with the 0.6.3 readings beside them: the tables above stay on the 0.3.1 runs, and
the gain the same new runs gave the app in front is shown beside them.

    python docs/bench/summary.py          # rewrites the tables in README.md
"""
import json
import statistics
from pathlib import Path

HERE = Path(__file__).resolve().parent
README = HERE.parent.parent / "README.md"
START, END = "<!-- bench:start -->", "<!-- bench:end -->"
COST_START, COST_END = "<!-- bench-cost:start -->", "<!-- bench-cost:end -->"
CHANGE_START, CHANGE_END = "<!-- bench-change:start -->", "<!-- bench-change:end -->"
SAVER_START, SAVER_END = "<!-- bench-saver:start -->", "<!-- bench-saver:end -->"
NAMES = {
    "windows-latest": "Windows, x64",
    "macos-15": "macOS 15, Apple silicon",
    "macos-15-intel": "macOS 15, Intel",
    "ubuntu-latest": "Linux (Ubuntu), x64",
    "windows-latest-2threads": "Windows, x64, on 2 threads only",
    "ubuntu-latest-2threads": "Linux (Ubuntu), x64, on 2 threads only",
    "ubuntu-24.04-arm": "Linux (Ubuntu), ARM64",
    "ubuntu-24.04-arm-2threads": "Linux (Ubuntu), ARM64, on 2 threads only",
}
# The same machines with two busy background processes only, so with processor to spare.
# Measured from 0.6.3 on; these rows are in the Saver table and nowhere else.
FEW = {
    "windows-latest-2busy": "Windows, x64",
    "macos-15-2busy": "macOS 15, Apple silicon",
    "macos-15-intel-2busy": "macOS 15, Intel",
    "ubuntu-latest-2busy": "Linux (Ubuntu), x64",
    "ubuntu-24.04-arm-2busy": "Linux (Ubuntu), ARM64",
}
V01, V02, V06, V064 = HERE / "v0.1", HERE / "v0.2", HERE / "v0.6", HERE / "v0.6.4"
med = statistics.median


def load(folder=HERE):
    runs = {}
    for path in sorted(folder.glob("run-*/*.json")):
        runs.setdefault(path.stem, []).append(json.loads(path.read_text(encoding="utf-8")))
    return runs


def hold_back(folder=HERE):
    """Milliseconds until the hold was back, per system, one value per run."""
    times = {}
    for path in sorted(folder.glob("run-*/*-hold-back-ms.txt")):
        times.setdefault(path.name.removesuffix("-hold-back-ms.txt"), []).append(int(path.read_text().strip()))
    return times


def gains(results, phase="eased"):
    """Gain over Spry off per run. 'eased' is the default; 'pulsed' is Strong mode."""
    return [r[phase]["work"] / r["busy"]["work"] for r in results if r.get(phase)]


def row(name, results):
    quiet = med([r["quiet"]["work"] for r in results])
    busy = med([r["busy"]["work"] for r in results])
    with_spry = med([r["eased"]["work"] for r in results])
    gain = gains(results)
    of_quiet = med([r["eased"]["work"] / r["quiet"]["work"] for r in results])
    pulsed = gains(results, "pulsed")
    p95_off = med([r["busy"]["p95_ms"] for r in results])
    p95_on = med([r["eased"]["p95_ms"] for r in results])
    cores = results[0]["cores"]
    kept = results[0].get("kept_free", 0)
    return (
        f"| {NAMES.get(name, name)} | {len(results)} | {cores} | {kept or 'none'} | {quiet:.0f} | {busy:.0f} | {with_spry:.0f} "
        f"| **{med(gain):.2f}x** ({min(gain):.2f}-{max(gain):.2f}) | {100 * of_quiet:.0f}% "
        f"| {(f'{med(pulsed):.2f}x' if pulsed else 'not offered')} "
        f"| {p95_off:.1f} -> {p95_on:.1f} ms |"
    )


def run_links(folder=HERE):
    ids = sorted({p.parent.name.removeprefix("run-") for p in folder.glob("run-*/*.json")})
    return ", ".join(f"[{i}](https://github.com/juw12/spry/actions/runs/{i})" for i in ids)


def table():
    runs = load()
    lines = [
        "| System | Runs | Threads | Kept free | Quiet | Busy, Spry off | Busy, Spry on | Gain (lowest-highest) | Of quiet speed | With Strong mode | Slowest 5% of reactions, off -> on |",
        "|---|---|---|---|---|---|---|---|---|---|---|",
    ]
    lines += [row(name, runs[name]) for name in NAMES if name in runs]
    lines.append("")
    lines.append(f"Each row is the median over its CI runs ({run_links()}). Work is in steps per second.")
    return "\n".join(lines)


def shares(results, way):
    """Speed the held apps kept while the app in front rested, per run, with Spry off = 1.
    The bench takes the ways in turns and reports the middle round of five."""
    return [r["background"][way] for r in results if r["background"].get(way) is not None]


def cost_row(name, results, times):
    share = lambda values: f"{100 * med(values):.0f}%" if values else "not offered"
    spread = lambda values: f"({100 * min(values):.0f}-{100 * max(values):.0f})"
    works = [r["background"]["front_works_held"] / r["background"]["front_works_off"] for r in results]
    lent = shares(results, "rest_share_lent")
    back = times.get(name)
    return (
        f"| {NAMES.get(name, name)} | {share(works)} {spread(works)} "
        f"| {share(shares(results, 'rest_share_held'))} "
        f"| {share(shares(results, 'rest_share_pulsed'))} "
        f"| **{share(lent)}** {spread(lent)} "
        f"| {(f'{med(back) / 1000:.1f} s ({max(back) / 1000:.1f} s)' if back else 'timed on all threads only')} |"
    )


def cost():
    """What the hold costs the held apps: their own speed, with Spry off = 100%."""
    runs = {name: results for name, results in load().items() if all("background" in r for r in results)}
    times = hold_back()
    lines = [
        "| System | Front app works: held (lowest-highest) | Front app rests: still held (0.2; Saver mode up to 0.6.3) | The same with pulses (0.2 on Linux) | Front app rests: lent (Performance mode, lowest-highest) | Hold back after (slowest) |",
        "|---|---|---|---|---|---|",
    ]
    lines += [cost_row(name, runs[name], times) for name in NAMES if name in runs]
    return "\n".join(lines)


def change():
    """The gain for the app in front, version by version: same runners, same test, easing only."""
    v01, v02, now = load(V01), load(V02), load()
    cell = lambda runs, name, phase="eased": f"{med(gains(runs[name], phase)):.2f}x" if gains(runs.get(name, []), phase) else "-"
    lines = [
        "| System | 0.1.1 | 0.2.0 | 0.3.1 (now) | With pulses, 0.2.0 | With pulses, now |",
        "|---|---|---|---|---|---|",
    ]
    for name in NAMES:
        if name in v02 and name in now:
            lines.append(
                f"| {NAMES[name]} | {cell(v01, name)} | {cell(v02, name)} | {cell(now, name)} "
                f"| {cell(v02, name, 'pulsed')} | {cell(now, name, 'pulsed')} |"
            )
    return "\n".join(lines)


def busy(results, state):
    """Percent of the whole computer in use while the app in front rested, per run."""
    return [r["background"][state] for r in results if r["background"].get(state) is not None]


def saver_row(name, results, before, names=NAMES):
    held = shares(results, "rest_share_held")
    off, on = busy(results, "rest_busy_off"), busy(results, "rest_busy_held")
    gain = gains(results)
    cores, few = results[0]["cores"], results[0].get("few", 0)
    whole = f"{med(off):.0f}% -> **{med(on):.0f}%** ({min(on):.0f}-{max(on):.0f})" if off and on else "not read"
    was_held, was_on = shares(before, "rest_share_held"), busy(before, "rest_busy_held")
    was = f"{100 * med(was_held):.0f}%, {med(was_on):.0f}% busy" if was_held and was_on else "not measured"
    return (
        f"| {names.get(name, name)} | {len(results)} | {cores} | {results[0]['burners']} | {f'{few} of {cores}' if few else 'all'} "
        f"| **{100 * med(held):.0f}%** ({100 * min(held):.0f}-{100 * max(held):.0f}) | {whole} "
        f"| {was} | {med(gain):.2f}x ({min(gain):.2f}-{max(gain):.2f}) |"
    )


SAVER_HEAD = [
    "| System | Runs | Threads | Busy processes | Threads the held apps run on | Work the held apps get done, Spry off = 100% (lowest-highest) | Whole computer busy, off -> Saver (lowest-highest) | The same two on 0.6.3 | Gain for the app in front in the same runs (lowest-highest) |",
    "|---|---|---|---|---|---|---|---|---|",
]


def saver():
    """Saver mode while the app in front rests: what the held apps still get done, and how busy
    the whole computer is. In Saver mode they stay held like this for as long as they are in the
    background. Two tables: the overloaded machine of the other tables, and the same machines
    with two busy processes only. Beside each row: what 0.6.3 read on the same kind of runner,
    before held apps were kept to about a quarter of the threads."""
    runs, old = load(V064), load(V06)
    full = [name for name in NAMES if name in runs]
    few = [name for name in FEW if name in runs]
    lines = ["**More busy processes than processor threads** (the load of the tables above):", ""]
    lines += SAVER_HEAD + [saver_row(name, runs[name], old.get(name, [])) for name in full]
    if few:
        lines += ["", "**Two busy processes only**, so with processor to spare:", ""]
        lines += SAVER_HEAD + [saver_row(name, runs[name], old.get(name, []), FEW) for name in few]
    lines.append("")
    count = lambda names, kept=runs: sorted({len(kept[name]) for name in names if name in kept})
    spread = lambda counts: str(counts[0]) if len(counts) == 1 else f"{counts[0]} to {counts[-1]}"
    lines.append(
        f"Each row is the median over its CI runs of the 0.6.4 code ({run_links(V064)}): "
        f"{spread(count(full + few))} runs per row. "
        f"The 0.6.3 column is the median over the runs of the 0.6.3 code ({run_links(V06)}): "
        f"{spread(count(full, old))} runs per row in the first table, {spread(count(few, old))} in the second."
    )
    return "\n".join(lines)


def between(text, start, end, body):
    head, rest = text.split(start, 1)
    _, tail = rest.split(end, 1)
    return head + start + "\n" + body + "\n" + end + tail


if __name__ == "__main__":
    text = README.read_text(encoding="utf-8")
    tables = [(START, END, table), (COST_START, COST_END, cost), (CHANGE_START, CHANGE_END, change), (SAVER_START, SAVER_END, saver)]
    for start, end, build in tables:
        if start in text:
            text = between(text, start, end, build())
        print(build(), "\n")
    README.write_bytes(text.encode("utf-8"))
