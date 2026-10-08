"""Compare NodeView.type lookups and filters in two frozen Python source trees.

Both trees must contain python/ironweaver and the same compiled extension.
Example: python benchmarks/compare_nodeview_type.py BASELINE CANDIDATE --output results.json
"""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import random
import statistics
import subprocess
import sys
import timeit


def worker():
    import ironweaver
    from ironweaver import NodeView, Vertex

    samples = {}
    for width in (0, 8, 64, 512):
        attrs = {f"property_{i}": i for i in range(width)}
        graph = Vertex()
        for i in range(1000):
            graph.add_node(str(i), {**attrs, "type": "selected" if i % 100 == 0 else "other"})
        view = NodeView(graph.get_node("0"))
        assert view.type == "selected"
        operations = {f"lookup/{width}": (lambda: view.type, 20000)}
        if width in (8, 512):
            operations.update({
                f"filter-none/{width}": (lambda: graph.filter(lambda n: n.type == "absent"), 10),
                f"filter-selective/{width}": (lambda: graph.filter(lambda n: n.type == "selected"), 10),
                f"control-attr/{width}": (lambda: graph.filter(lambda n: n.attr("property_0") < 0), 10),
            })
            assert graph.filter(lambda n: n.type == "absent").node_count() == 0
            assert {n.id for n in graph.filter(lambda n: n.type == "selected")} == {
                str(i) for i in range(0, 1000, 100)
            }
            assert graph.filter(lambda n: n.attr("property_0") < 0).node_count() == 0
        for name, (operation, number) in operations.items():
            operation()
            samples[name] = [t / number for t in timeit.repeat(operation, number=number, repeat=3)]

    package = Path(ironweaver.__file__).parent
    extension = next(package.glob("_ironweaver*.so"))
    return {
        "python": sys.version,
        "affinity": sorted(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") else None,
        "extension_sha256": hashlib.sha256(extension.read_bytes()).hexdigest(),
        "wrapper_sha256": hashlib.sha256((package / "__init__.py").read_bytes()).hexdigest(),
        "samples_seconds": samples,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", nargs="?")
    parser.add_argument("candidate", nargs="?")
    parser.add_argument("--pairs", type=int, default=12)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--worker", action="store_true")
    args = parser.parse_args()
    if args.worker:
        print(json.dumps(worker()))
        return
    if not args.baseline or not args.candidate or not args.output or args.pairs < 2:
        parser.error("provide baseline, candidate, --output, and at least two pairs")
    if hasattr(os, "sched_setaffinity"):
        os.sched_setaffinity(0, {min(os.sched_getaffinity(0))})
    records = []
    for pair in range(args.pairs):
        record = {}
        order = ("baseline", "candidate") if pair % 2 == 0 else ("candidate", "baseline")
        for revision in order:
            env = dict(os.environ, PYTHONPATH=str(Path(getattr(args, revision)).resolve() / "python"),
                       PYTHONHASHSEED="0", RAYON_NUM_THREADS="1")
            record[revision] = json.loads(subprocess.check_output(
                [sys.executable, str(Path(__file__).resolve()), "--worker"], env=env, text=True
            ))
        records.append(record)
        print(f"Completed pair {pair + 1}/{args.pairs}", flush=True)
    for field in ("python", "affinity", "extension_sha256"):
        assert all(record[revision][field] == records[0]["baseline"][field]
                   for record in records for revision in ("baseline", "candidate")), field
    for revision in ("baseline", "candidate"):
        assert len({record[revision]["wrapper_sha256"] for record in records}) == 1

    summary = {}
    rng = random.Random(0)
    for name in records[0]["baseline"]["samples_seconds"]:
        baseline = [statistics.median(r["baseline"]["samples_seconds"][name]) for r in records]
        candidate = [statistics.median(r["candidate"]["samples_seconds"][name]) for r in records]
        log_ratios = [math.log(b / c) for b, c in zip(baseline, candidate)]
        bootstrap = sorted(math.exp(statistics.mean(rng.choices(log_ratios, k=len(log_ratios))))
                           for _ in range(10000))
        summary[name] = {
            "baseline_median_seconds": statistics.median(baseline),
            "candidate_median_seconds": statistics.median(candidate),
            "paired_speedup": math.exp(statistics.mean(log_ratios)),
            "bootstrap_95_percent_interval": [bootstrap[250], bootstrap[9749]],
        }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps({"pairs": records, "summary": summary}, indent=2) + "\n")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
