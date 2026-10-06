"""Compare two release bench_remove_node binaries in alternating paired runs.

Build each revision with the same toolchain, lockfile and release flags, and
copy target/release/examples/bench_remove_node to separate paths. Then run:

  python benchmarks/compare_remove_node.py /tmp/baseline /tmp/candidate --output /tmp/removal.json

Graph construction, cloning, assertions and destruction are excluded from
Rust timings. Each process warms up first; pairs alternate AB/BA to reduce
time drift. On Linux both binaries inherit affinity to one available CPU.
Bootstrap intervals resample process pairs, not correlated inner samples.
The JSON retains every sample. Run without concurrent builds or tests.
"""
from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
import math
import os
from pathlib import Path
import platform
import random
import statistics
import subprocess

CASES = [
    ("parallel-out", 1000), ("parallel-out", 4000), ("parallel-out", 16000),
    ("parallel-in", 16000), ("bidirectional", 16000), ("mixed", 16000),
    ("star", 16000), ("shuffled-star", 16000), ("star", 4),
    ("self-loops", 16000), ("isolated", 0), ("leaf-out", 16000), ("leaf-in", 16000),
]


def run(binary, shape, edges, samples, batch):
    result = subprocess.run(
        [str(binary), shape, str(edges), str(samples), str(batch)],
        check=True, capture_output=True, text=True,
    )
    rows = list(csv.DictReader(io.StringIO(result.stdout)))
    assert len(rows) == samples
    assert all(row["shape"] == shape and int(row["edges"]) == edges for row in rows)
    values = [float(row["ns_per_delete"]) for row in rows]
    assert all(math.isfinite(x) and x > 0 for x in values)
    return values


def summarize(pairs):
    before = [statistics.median(p["baseline"]) for p in pairs]
    after = [statistics.median(p["candidate"]) for p in pairs]
    ratios = [math.log(b / a) for b, a in zip(before, after)]
    rng = random.Random(42)
    boot = sorted(math.exp(statistics.mean(rng.choices(ratios, k=len(ratios)))) for _ in range(10000))
    return {
        "baseline_median_ns": statistics.median(before),
        "candidate_median_ns": statistics.median(after),
        "baseline_range_ns": [min(before), max(before)],
        "candidate_range_ns": [min(after), max(after)],
        "paired_geomean_speedup": math.exp(statistics.mean(ratios)),
        "speedup_95pct_ci": [boot[250], boot[9749]],
        "candidate_faster_pairs": sum(a < b for b, a in zip(before, after)),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("--pairs", type=int, default=20)
    parser.add_argument("--samples", type=int, default=3)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.pairs < 2 or args.samples < 1:
        parser.error("need at least two pairs and one sample")
    cpu = None
    if hasattr(os, "sched_getaffinity"):
        cpu = min(os.sched_getaffinity(0))
        os.sched_setaffinity(0, {cpu})
    binaries = {"baseline": args.baseline.resolve(), "candidate": args.candidate.resolve()}
    report = {"platform": platform.platform(), "cpu_affinity": cpu,
              "binaries": {k: str(v) for k, v in binaries.items()},
              "binary_sha256": {k: hashlib.sha256(v.read_bytes()).hexdigest() for k, v in binaries.items()},
              "pairs": args.pairs, "samples_per_process": args.samples,
              "warmup_samples": 3, "results": []}
    for shape, edges in CASES:
        batch = 1000 if edges <= 4 else 4
        pairs = []
        for i in range(args.pairs):
            order = ("baseline", "candidate") if i % 2 == 0 else ("candidate", "baseline")
            pair = {"order": list(order)}
            for name in order:
                pair[name] = run(binaries[name], shape, edges, args.samples, batch)
            pairs.append(pair)
        summary = summarize(pairs)
        report["results"].append({"shape": shape, "edges": edges, "batch": batch,
                                  "summary": summary, "raw_pairs": pairs})
        args.output.write_text(json.dumps(report, indent=2) + "\n")
        lo, hi = summary["speedup_95pct_ci"]
        print(f"{shape:14} {edges:6}: "
              f"{summary['baseline_median_ns']/1000:10.3f} -> "
              f"{summary['candidate_median_ns']/1000:10.3f} us; "
              f"{summary['paired_geomean_speedup']:.2f}x [{lo:.2f}, {hi:.2f}]", flush=True)


if __name__ == "__main__":
    main()
