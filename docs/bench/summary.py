"""Builds the measurement table in README.md from the CI results kept in docs/bench/.

Each folder docs/bench/run-<id>/ holds what `spry bench --secs 4 --json` printed on the
GitHub-hosted runners in CI run <id> (and again with `--threads 2` where the system allows it). The table shows the median over all kept runs and the
lowest and highest gain seen, so nothing in the README is typed by hand. The folder v0.1/ holds
the runs of version 0.1.1, which had no kept-free processor threads and timed reactions from a
timer; they are shown next to the current ones, never mixed with them.

    python docs/bench/summary.py          # rewrites the table in README.md
"""
import json
import statistics
from pathlib import Path

HERE = Path(__file__).resolve().parent
README = HERE.parent.parent / "README.md"
START, END = "<!-- bench:start -->", "<!-- bench:end -->"
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
CHANGE_START, CHANGE_END = "<!-- bench-change:start -->", "<!-- bench-change:end -->"
BEFORE = HERE / "v0.1"


def load(folder=HERE):
    runs = {}
    for path in sorted(folder.glob("run-*/*.json")):
        runs.setdefault(path.stem, []).append(json.loads(path.read_text(encoding="utf-8")))
    return runs


def row(name, results):
    med = lambda values: statistics.median(values)
    quiet = med([r["quiet"]["work"] for r in results])
    busy = med([r["busy"]["work"] for r in results])
    best = []
    for r in results:
        phases = [r["eased"]] + ([r["pulsed"]] if r.get("pulsed") else [])
        best.append(max(phases, key=lambda p: p["work"]))
    with_spry = med([b["work"] for b in best])
    gains = [b["work"] / r["busy"]["work"] for b, r in zip(best, results)]
    of_quiet = med([b["work"] / r["quiet"]["work"] for b, r in zip(best, results)])
    eased_gain = med([r["eased"]["work"] / r["busy"]["work"] for r in results])
    pulsed = [r["pulsed"]["work"] / r["busy"]["work"] for r in results if r.get("pulsed")]
    p95_off = med([r["busy"]["p95_ms"] for r in results])
    p95_on = med([b["p95_ms"] for b in best])
    cores = results[0]["cores"]
    kept = results[0].get("kept_free", 0)
    return (
        f"| {NAMES.get(name, name)} | {len(results)} | {cores} | {kept or 'none'} | {quiet:.0f} | {busy:.0f} | {with_spry:.0f} "
        f"| **{med(gains):.2f}x** ({min(gains):.2f}-{max(gains):.2f}) | {100 * of_quiet:.0f}% "
        f"| {eased_gain:.2f}x | {(f'{med(pulsed):.2f}x' if pulsed else 'not used')} "
        f"| {p95_off:.1f} -> {p95_on:.1f} ms |"
    )


def table():
    runs = load()
    lines = [
        "| System | Runs | Threads | Kept free | Quiet | Busy, Spry off | Busy, Spry on | Gain (lowest-highest) | Of quiet speed | Easing only | With pulses | Slowest 5% of reactions, off -> on |",
        "|---|---|---|---|---|---|---|---|---|---|---|---|",
    ]
    lines += [row(name, runs[name]) for name in NAMES if name in runs]
    ids = sorted({p.parent.name.removeprefix("run-") for p in HERE.glob("run-*/*.json")})
    links = ", ".join(f"[{i}](https://github.com/juw12/spry/actions/runs/{i})" for i in ids)
    lines.append("")
    lines.append(f"Each row is the median over its CI runs ({links}). Work is in steps per second.")
    return "\n".join(lines)


def gain(results, phase):
    """Median gain over Spry off; phase is 'eased', or 'best' for the stronger of eased and pulsed."""
    values = []
    for r in results:
        work = r["eased"]["work"]
        if phase == "best" and r.get("pulsed"):
            work = max(work, r["pulsed"]["work"])
        values.append(work / r["busy"]["work"])
    return statistics.median(values)


def change():
    """Version 0.1.1 next to this version, same runners, same test."""
    before, now = load(BEFORE), load()
    lines = [
        "| System | Gain, 0.1.1 | Gain, now | Without pausing, 0.1.1 | Without pausing, now |",
        "|---|---|---|---|---|",
    ]
    for name in NAMES:
        if name in before and name in now:
            lines.append(
                f"| {NAMES[name]} | {gain(before[name], 'best'):.2f}x | {gain(now[name], 'best'):.2f}x "
                f"| {gain(before[name], 'eased'):.2f}x | {gain(now[name], 'eased'):.2f}x |"
            )
    return "\n".join(lines)


def between(text, start, end, body):
    head, rest = text.split(start, 1)
    _, tail = rest.split(end, 1)
    return head + start + "\n" + body + "\n" + end + tail


if __name__ == "__main__":
    text = between(README.read_text(encoding="utf-8"), START, END, table())
    if CHANGE_START in text:
        text = between(text, CHANGE_START, CHANGE_END, change())
    README.write_bytes(text.encode("utf-8"))
    print(table())
    print(change())
