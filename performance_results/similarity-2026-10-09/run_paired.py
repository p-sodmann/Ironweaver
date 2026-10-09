"""Measure frozen before/after release executables in alternating order."""
import argparse
import csv
import hashlib
import io
import json
import math
import os
from pathlib import Path
import statistics
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--pairs", type=int, default=6)
    parser.add_argument("--cpu", type=int, default=0)
    parser.add_argument("--filter")
    parser.add_argument("--milliseconds", type=int, default=200)
    args = parser.parse_args()
    assert args.pairs == 6, "The confidence interval below uses t(5) = 2.57058."
    args.output.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, RAYON_NUM_THREADS="1", IW_BENCH_MS=str(args.milliseconds))
    env.pop("IW_BENCH_FILTER", None)
    if args.filter:
        env["IW_BENCH_FILTER"] = args.filter
    binaries = {"before": args.before.resolve(), "after": args.after.resolve()}
    metadata = {
        "base_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "binaries_sha256": {k: hashlib.sha256(p.read_bytes()).hexdigest() for k, p in binaries.items()},
        "cargo_lock_sha256": hashlib.sha256(Path("Cargo.lock").read_bytes()).hexdigest(),
        "pairs": args.pairs,
        "cpu": args.cpu,
        "rayon_threads": 1,
        "sample_duration_ms": args.milliseconds,
        "filter": args.filter,
        "build": "Rust 1.99.0, Cargo release profile, offline, identical Cargo.lock",
    }
    (args.output / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    samples = []
    for pair in range(args.pairs):
        order = ["before", "after"] if pair % 2 == 0 else ["after", "before"]
        current = {}
        for label in order:
            result = subprocess.run(
                ["taskset", "-c", str(args.cpu), str(binaries[label])],
                env=env, text=True, capture_output=True, check=True,
            )
            (args.output / f"pair-{pair + 1:02}-{label}.csv").write_text(result.stdout)
            current[label] = {r["workload"]: float(r["seconds_per_call"]) for r in csv.DictReader(io.StringIO(result.stdout))}
            assert current[label]
            if not args.filter:
                assert len(current[label]) == 24
            else:
                assert all(args.filter in name for name in current[label])
            print(f"pair {pair + 1}/{args.pairs}: {label} finished", flush=True)
        assert current["before"].keys() == current["after"].keys()
        samples.append(current)
    rows = []
    for name in samples[0]["before"]:
        before = [s["before"][name] for s in samples]
        after = [s["after"][name] for s in samples]
        log_ratios = [math.log(b / a) for a, b in zip(before, after)]
        mean = statistics.mean(log_ratios)
        margin = 2.57058 * statistics.stdev(log_ratios) / math.sqrt(args.pairs)
        rows.append({
            "workload": name,
            "before_ms": statistics.mean(before) * 1000,
            "after_ms": statistics.mean(after) * 1000,
            "speedup": math.exp(-mean),
            "change_percent": 100 * math.expm1(mean),
            "ci_low_percent": 100 * math.expm1(mean - margin),
            "ci_high_percent": 100 * math.expm1(mean + margin),
        })
    with (args.output / "summary.csv").open("w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)
    lines = [
        "| Hub degree / other degree / position / metric | Before (ms) | After (ms) | Change [95% CI] |",
        "|---|---:|---:|---:|",
    ]
    for r in rows:
        lines.append(
            f"| {r['workload']} | {r['before_ms']:.3f} | {r['after_ms']:.3f} | "
            f"{r['change_percent']:+.1f}% [{r['ci_low_percent']:+.1f}, {r['ci_high_percent']:+.1f}] |"
        )
    (args.output / "table.md").write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
