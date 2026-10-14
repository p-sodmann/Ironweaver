"""Projection.shortest_paths: "astar" versus "dijkstra" on a grid.

The default grid is about the size of the one in issue #113 (180k tiles,
716k directed edges): a 4-connected grid with random obstacles, where the
edge u -> v costs entering v: 1 for most tiles, 2 to 4 for a fraction
`--rough` of them. Every cost is at least 1, so the Manhattan distance never
overestimates. The projection is built with Projection.from_arrays,
coordinates included. Prints the median time per trip of each method (one
call per trip, and one batch call for all trips) and checks that all find
paths of the same cost.

Set RAYON_NUM_THREADS=1 to time a single thread (the batch call runs the
trips in parallel otherwise).

    python benchmarks/compare_astar.py [--side 430] [--obstacles 0.025] [--rough 0.2] [--trips 200]
"""

import argparse
import gc
import os
import statistics
import time

import numpy as np

from ironweaver import Projection


def grid(side, obstacles, rough, seed):
    """Free cells of a side x side grid, edges both ways between adjacent
    free cells weighing the cost of the cell entered, and the cells'
    (column, row) coordinates."""
    rng = np.random.default_rng(seed)
    free = rng.random((side, side)) >= obstacles
    index = np.full((side, side), -1, dtype=np.int64)
    index[free] = np.arange(free.sum())
    rows, cols = np.nonzero(free)
    ids = [f"{r},{c}" for r, c in zip(rows.tolist(), cols.tolist())]
    tile_cost = np.where(rng.random(len(ids)) < rough, rng.integers(2, 5, len(ids)), 1).astype(np.float64)
    src, dst = [], []
    for a, b in [(index[:, :-1], index[:, 1:]), (index[:-1, :], index[1:, :])]:
        ok = (a >= 0) & (b >= 0)
        src += [a[ok], b[ok]]
        dst += [b[ok], a[ok]]
    src, dst = np.concatenate(src), np.concatenate(dst)
    coords = np.stack([cols, rows], axis=1).astype(np.float64)
    return ids, src, dst, tile_cost[dst], coords


def per_trip(f, trips, repeats):
    """Median seconds per trip over `repeats` runs of f()."""
    times = []
    for _ in range(repeats):
        gc.collect()
        t = time.perf_counter()
        f()
        times.append((time.perf_counter() - t) / trips)
    return statistics.median(times)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--side", type=int, default=430)
    ap.add_argument("--obstacles", type=float, default=0.025)
    ap.add_argument("--rough", type=float, default=0.2)
    ap.add_argument("--trips", type=int, default=200)
    ap.add_argument("--repeats", type=int, default=5)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--stats", action="store_true", help="include and report core settled-node counts")
    args = ap.parse_args()

    ids, src, dst, weight, coords = grid(args.side, args.obstacles, args.rough, args.seed)
    proj = Projection.from_arrays(ids, src, dst, weight, coords)
    rng = np.random.default_rng(args.seed + 1)
    pairs = [(ids[a], ids[b]) for a, b in rng.integers(0, len(ids), (args.trips, 2)).tolist()]
    threads = os.environ.get("RAYON_NUM_THREADS", "all")
    print(f"{len(ids):,} nodes, {len(src):,} edges, {args.trips} random trips, "
          f"median of {args.repeats}, threads: {threads}")

    methods = {
        "dijkstra": {"method": "dijkstra"},
        "astar manhattan": {"method": "astar", "heuristic": "manhattan"},
        "astar euclidean": {"method": "astar", "heuristic": "euclidean"},
    }
    if args.stats:
        for kw in methods.values():
            kw["stats"] = True
    results = {name: proj.shortest_paths(pairs, **kw) for name, kw in methods.items()}
    want = [r and r["cost"] for r in results["dijkstra"]]
    for name, got in results.items():
        assert [r and r["cost"] for r in got] == want, f"{name} costs differ from dijkstra"

    print(f"  {'method':<17} {'one call/trip':>14} {'batch':>12}")
    base = None
    for name, kw in methods.items():
        single = per_trip(lambda: [proj.shortest_paths([p], **kw) for p in pairs], len(pairs), args.repeats)
        batch = per_trip(lambda: proj.shortest_paths(pairs, **kw), len(pairs), args.repeats)
        base = base or single
        print(f"  {name:<17} {1000 * single:11.3f} ms {1000 * batch:9.3f} ms   ({base / single:.1f}x)")
        if args.stats:
            counts = [r["stats"]["settled"] for r in results[name] if r is not None]
            print(f"    median nodes settled: {statistics.median(counts):,.0f}")


if __name__ == "__main__":
    main()
