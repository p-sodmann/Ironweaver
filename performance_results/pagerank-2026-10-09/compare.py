"""Alternate two frozen benchmark binaries; verify results and retain all samples."""

import argparse
import json
import os
from pathlib import Path
import statistics
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    parser.add_argument("--output", type=Path, default=Path("samples.json"))
    parser.add_argument("--cpus", help="Linux taskset CPU list, e.g. 0,1,2; omit on macOS")
    parser.add_argument("--threads", type=int, nargs="+", default=[1, 3])
    parser.add_argument("--pairs", type=int, default=4)
    parser.add_argument("--nodes", type=int, default=100_000)
    args = parser.parse_args()
    records = []
    for threads in args.threads:
        for pair in range(args.pairs):
            order = ("before", "after") if pair % 2 == 0 else ("after", "before")
            for version in order:
                env = dict(os.environ, RAYON_NUM_THREADS=str(threads), IWDB_BENCH_NODES=str(args.nodes), SAMPLES="5", SAMPLE_MS="300")
                command = [str(getattr(args, version).resolve())]
                if args.cpus:
                    cpus = ",".join(args.cpus.split(",")[:threads])
                    command = ["taskset", "-c", cpus] + command
                output = subprocess.check_output(command, env=env, text=True)
                for line in output.splitlines():
                    records.append(dict(json.loads(line), version=version, pair=pair))
                args.output.write_text(json.dumps(records, indent=2) + "\n")
                print(f"threads={threads} pair={pair} {version} complete", flush=True)
    for threads in args.threads:
        for case in ("db-default", "unit-20", "weighted-20"):
            rows = [r for r in records if r["threads"] == threads and r["case"] == case]
            assert len({r["fingerprint"] for r in rows}) == 1, (threads, case, "rank mismatch")
            assert len({r["iterations"] for r in rows}) == 1, (threads, case, "iteration mismatch")
            medians = {v: statistics.median(t for r in rows if r["version"] == v for t in r["samples_us"]) for v in ("before", "after")}
            ratios = []
            for pair in range(args.pairs):
                paired = {r["version"]: statistics.median(r["samples_us"]) for r in rows if r["pair"] == pair}
                ratios.append(100 * (1 - paired["after"] / paired["before"]))
            print(json.dumps(dict(threads=threads, case=case, medians_us=medians, improvement_percent=100*(1-medians["after"]/medians["before"]), paired_improvements_percent=ratios)), flush=True)


if __name__ == "__main__":
    main()
