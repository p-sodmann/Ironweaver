"""Projection.set_weights versus rebuilding the projection, on a grid.

The grid is the one of compare_astar.py (about 180k tiles and 716k directed
edges by default, coordinates included). A "building" makes the 4 edges into
a random tile impassable (weight inf) and a later change restores them;
bigger batches change the edges into more tiles at once. Prints the median
time of one set_weights call per batch size (with and without the transposed
adjacency built, which set_weights updates too), next to the time of
rebuilding the projection with Projection.from_arrays, and checks that the
updated projection finds the same paths as a rebuilt one.

    python benchmarks/compare_set_weights.py [--side 430] [--obstacles 0.025] [--repeats 200]
"""

import argparse
import gc
import statistics
import time

import numpy as np

from compare_astar import grid
from ironweaver import Projection


def median_seconds(f, repeats):
    times = []
    for _ in range(repeats):
        t = time.perf_counter()
        f()
        times.append(time.perf_counter() - t)
    return statistics.median(times)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--side", type=int, default=430)
    ap.add_argument("--obstacles", type=float, default=0.025)
    ap.add_argument("--rough", type=float, default=0.2)
    ap.add_argument("--repeats", type=int, default=200)
    ap.add_argument("--seed", type=int, default=0)
    args = ap.parse_args()

    ids, src, dst, weight, coords = grid(args.side, args.obstacles, args.rough, args.seed)
    n = len(ids)
    # Edges into each tile (4 for most), to block / unblock whole tiles
    order = np.argsort(dst, kind="stable")
    starts = np.searchsorted(dst[order], np.arange(n + 1))
    rng = np.random.default_rng(args.seed + 1)

    def edges_into(tiles):
        return np.concatenate([order[starts[t]:starts[t + 1]] for t in tiles])

    gc.collect()
    rebuild = median_seconds(lambda: Projection.from_arrays(ids, src, dst, weight, coords), 5)
    print(f"{n:,} nodes, {len(src):,} edges; median of {args.repeats} calls (rebuild: of 5)")
    print(f"  {'rebuild (from_arrays)':<41}{1000 * rebuild:9.2f} ms")

    proj = Projection.from_arrays(ids, src, dst, weight, coords)
    for transposed in [False, True]:
        if transposed:
            proj.neighbors(ids[0], "in")  # builds the transposed adjacency
        for tiles in [1, 10, 100, 1000, 10000]:
            batches = [edges_into(rng.choice(n, tiles, replace=False)) for _ in range(args.repeats)]
            blocked = [np.full(len(b), np.inf) for b in batches]
            restored = [weight[b] for b in batches]
            it = iter(range(2 * args.repeats))

            def change():
                i = next(it)
                k = i // 2
                proj.set_weights(batches[k], blocked[k] if i % 2 == 0 else restored[k])

            t = median_seconds(change, 2 * args.repeats)
            edges = statistics.median(len(b) for b in batches)
            label = f"set_weights, {tiles:>5} tile{'s' if tiles > 1 else ' '} ({edges:,.0f} edges)"
            note = ", transposed built" if transposed else ""
            print(f"  {label:<41}{1000 * t:9.4f} ms   ({rebuild / t:,.0f}x faster than a rebuild{note})")

    # Same paths as a projection rebuilt with the changed weights
    tiles = rng.choice(n, 50, replace=False)
    blocked_edges = edges_into(tiles)
    proj.set_weights(blocked_edges, np.full(len(blocked_edges), np.inf))
    changed = weight.copy()
    changed[blocked_edges] = np.inf
    rebuilt = Projection.from_arrays(ids, src, dst, changed, coords)
    pairs = [(ids[a], ids[b]) for a, b in rng.integers(0, n, (50, 2)).tolist()]
    for method in ["dijkstra", "astar", "bfs"]:
        kw = {"heuristic": "manhattan"} if method == "astar" else {}
        assert proj.shortest_paths(pairs, method, **kw) == rebuilt.shortest_paths(pairs, method, **kw), method
    print("  paths after set_weights match a rebuilt projection")


if __name__ == "__main__":
    main()
