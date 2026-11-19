"""Builds the measurement table in README.md from the CI results kept in docs/bench/.

Each folder docs/bench/run-<id>/ holds what `spry bench --secs 4 --json` printed on the four
GitHub-hosted runners in CI run <id>. The table shows the median over all kept runs and the
lowest and highest gain seen, so nothing in the README is typed by hand.

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
}


def load():
    runs = {}
    for path in sorted(HERE.glob("run-*/*.json")):
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
    eased_gain = med([r["eased"]["work"] / r["busy"]["work"] for r in results])
    pulsed = [r["pulsed"]["work"] / r["busy"]["work"] for r in results if r.get("pulsed")]
    p95_off = med([r["busy"]["p95_ms"] for r in results])
    p95_on = med([b["p95_ms"] for b in best])
    cores = results[0]["cores"]
    return (
        f"| {NAMES.get(name, name)} | {cores} | {quiet:.0f} | {busy:.0f} | {with_spry:.0f} "
        f"| **{med(gains):.2f}x** ({min(gains):.2f}-{max(gains):.2f}) | {100 * with_spry / quiet:.0f}% "
        f"| {eased_gain:.2f}x | {(f'{med(pulsed):.2f}x' if pulsed else 'not used')} "
        f"| {p95_off:.1f} -> {p95_on:.1f} ms |"
    )


def table():
    runs = load()
    count = max(len(v) for v in runs.values())
    lines = [
        "| System | Cores | Quiet | Busy, Spry off | Busy, Spry on | Gain (lowest-highest) | Of quiet speed | Easing only | With pulses | Slowest 5% of steps, off -> on |",
        "|---|---|---|---|---|---|---|---|---|---|",
    ]
    lines += [row(name, runs[name]) for name in NAMES if name in runs]
    ids = sorted({p.parent.name.removeprefix("run-") for p in HERE.glob("run-*/*.json")})
    links = ", ".join(f"[{i}](https://github.com/juw12/spry/actions/runs/{i})" for i in ids)
    lines.append("")
    lines.append(f"Median of {count} CI run{'s' if count != 1 else ''} ({links}). Work is in steps per second.")
    return "\n".join(lines)


if __name__ == "__main__":
    text = README.read_text(encoding="utf-8")
    head, rest = text.split(START, 1)
    _, tail = rest.split(END, 1)
    README.write_bytes((head + START + "\n" + table() + "\n" + END + tail).encode("utf-8"))
    print(table())
