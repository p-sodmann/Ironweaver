"""Compare frozen binaries in old/new/old triplets and verify matching results."""

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
    parser.add_argument("--sizes", type=int, nargs="+", default=[100_000, 1_000_000])
    parser.add_argument("--triplets", type=int, default=4)
    parser.add_argument("--cpu", help="Linux taskset CPU, e.g. 0; omit on macOS")
    args = parser.parse_args()
    records = []
    for nodes in args.sizes:
        for triplet in range(args.triplets):
            for position, version in enumerate(("before", "after", "before")):
                env = dict(os.environ, IWDB_BENCH_NODES=str(nodes), VERIFY_GROUPS="32", SAMPLES="5", SAMPLE_MS="300")
                env.pop("CASE", None)
                if nodes == 1_000_000:
                    env["CASE"] = "db-two-hops-100"
                command = [str(getattr(args, version).resolve())]
                if args.cpu:
                    command = ["taskset", "-c", args.cpu] + command
                output = subprocess.check_output(command, env=env, text=True)
                for line in output.splitlines():
                    row = dict(json.loads(line), version=version, triplet=triplet, position=position)
                    previous = [r for r in records if r["nodes"] == nodes and r["case"] == row["case"]]
                    for key in ("fingerprint", "results", "matches", "truncated", "visited", "edges"):
                        assert all(r[key] == row[key] for r in previous), (nodes, row["case"], key)
                    records.append(row)
                args.output.write_text(json.dumps(records, indent=2) + "\n")
                print(f"nodes={nodes} triplet={triplet} position={position} {version} complete", flush=True)
    for nodes in args.sizes:
        cases = sorted({r["case"] for r in records if r["nodes"] == nodes})
        for case in cases:
            rows = [r for r in records if r["nodes"] == nodes and r["case"] == case]
            medians = {v: statistics.median(t for r in rows if r["version"] == v for t in r["samples_us"]) for v in ("before", "after")}
            improvements = []
            for triplet in range(args.triplets):
                sub = {r["position"]: statistics.median(r["samples_us"]) for r in rows if r["triplet"] == triplet}
                baseline = (sub[0] + sub[2]) / 2
                improvements.append(100 * (1 - sub[1] / baseline))
            print(json.dumps(dict(nodes=nodes, case=case, medians_us=medians, improvement_percent=100*(1-medians["after"]/medians["before"]), triplet_improvements_percent=improvements)), flush=True)


if __name__ == "__main__":
    main()
