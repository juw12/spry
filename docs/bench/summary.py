"""Builds the measurement tables in README.md from the CI results kept in docs/bench/.

Each folder docs/bench/run-<id>/ holds what `spry bench --secs 4 --json` printed on the
GitHub-hosted runners in CI run <id> (and again with `--threads 2` where the system allows it),
and <system>-hold-back-ms.txt: how long the whole-program test waited for the hold to come back
after the app in front started to work. The tables show the median over all kept runs and the
lowest and highest value seen, so nothing in the README is typed by hand.

Older versions are kept apart and never mixed in: v0.1/ (0.1.1: no kept-free processor threads,
reactions timed from a timer) and v0.2/ (0.2.0: apps were held whatever the app in front did,
and pulses were the default on Linux).

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
V01, V02 = HERE / "v0.1", HERE / "v0.2"
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


def run_links():
    ids = sorted({p.parent.name.removeprefix("run-") for p in HERE.glob("run-*/*.json")})
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
        "| System | Front app works: held (lowest-highest) | Front app rests: held anyway (0.2) | The same with pulses (0.2 on Linux) | Front app rests: lent (now, lowest-highest) | Hold back after (slowest) |",
        "|---|---|---|---|---|---|",
    ]
    lines += [cost_row(name, runs[name], times) for name in NAMES if name in runs]
    return "\n".join(lines)


def change():
    """The gain for the app in front, version by version: same runners, same test, easing only."""
    v01, v02, now = load(V01), load(V02), load()
    cell = lambda runs, name, phase="eased": f"{med(gains(runs[name], phase)):.2f}x" if gains(runs.get(name, []), phase) else "-"
    lines = [
        "| System | 0.1.1 | 0.2.0 | 0.3.0 (now) | With pulses, 0.2.0 | With pulses, now |",
        "|---|---|---|---|---|---|",
    ]
    for name in NAMES:
        if name in v02 and name in now:
            lines.append(
                f"| {NAMES[name]} | {cell(v01, name)} | {cell(v02, name)} | {cell(now, name)} "
                f"| {cell(v02, name, 'pulsed')} | {cell(now, name, 'pulsed')} |"
            )
    return "\n".join(lines)


def between(text, start, end, body):
    head, rest = text.split(start, 1)
    _, tail = rest.split(end, 1)
    return head + start + "\n" + body + "\n" + end + tail


if __name__ == "__main__":
    text = README.read_text(encoding="utf-8")
    for start, end, build in [(START, END, table), (COST_START, COST_END, cost), (CHANGE_START, CHANGE_END, change)]:
        if start in text:
            text = between(text, start, end, build())
        print(build(), "\n")
    README.write_bytes(text.encode("utf-8"))
