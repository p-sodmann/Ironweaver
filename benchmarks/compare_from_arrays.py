"""Build a Projection from numpy arrays (Projection.from_arrays) versus through
a Vertex (add_nodes + add_edges + project), on a grid with obstacles.

The default grid is about the size of the one in issue #114 (180k nodes,
716k directed edges, 4-neighbour moves both ways). Both routes start from
the same arrays (ids list, int64 sources / targets, float64 weights); the
Vertex route also has to build the (source_id, target_id) tuples. Prints the
median of `--repeats` runs per step and checks both projections agree.

    python benchmarks/compare_from_arrays.py [--side 430] [--obstacles 0.025]
"""

import argparse
import gc
import statistics
import time

import numpy as np

from ironweaver import Projection, Vertex


def grid(side, obstacles, seed):
    """Free cells of a side x side grid, and edges both ways between
    horizontally / vertically adjacent free cells, as arrays."""
    rng = np.random.default_rng(seed)
    free = rng.random((side, side)) >= obstacles
    index = np.full((side, side), -1, dtype=np.int64)
    index[free] = np.arange(free.sum())
    rows, cols = np.nonzero(free)
    ids = [f"{r},{c}" for r, c in zip(rows.tolist(), cols.tolist())]
    src, dst = [], []
    for a, b in [(index[:, :-1], index[:, 1:]), (index[:-1, :], index[1:, :])]:
        ok = (a >= 0) & (b >= 0)
        src += [a[ok], b[ok]]
        dst += [b[ok], a[ok]]
    src, dst = np.concatenate(src), np.concatenate(dst)
    weight = 1.0 + rng.random(len(src))
    return ids, src, dst, weight


def timed(f):
    gc.collect()
    t = time.perf_counter()
    out = f()
    return time.perf_counter() - t, out


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--side", type=int, default=430)
    ap.add_argument("--obstacles", type=float, default=0.025)
    ap.add_argument("--repeats", type=int, default=5)
    ap.add_argument("--seed", type=int, default=0)
    args = ap.parse_args()

    ids, src, dst, weight = grid(args.side, args.obstacles, args.seed)
    print(f"{len(ids):,} nodes, {len(src):,} edges, best of {args.repeats} (median)")

    steps = {k: [] for k in ["tuples", "add_nodes", "add_edges", "project", "vertex total", "from_arrays"]}
    for _ in range(args.repeats):
        t_tuples, pairs = timed(lambda: list(zip([ids[i] for i in src.tolist()], [ids[i] for i in dst.tolist()])))
        g = Vertex()
        t_nodes, _ = timed(lambda: g.add_nodes(ids))
        t_edges, _ = timed(lambda: g.add_edges(pairs, attrs={"weight": weight.tolist()}))
        t_project, via_vertex = timed(lambda: g.project(weight="weight"))
        del g, pairs
        t_arrays, direct = timed(lambda: Projection.from_arrays(ids, src, dst, weight))
        for k, v in zip(steps, [t_tuples, t_nodes, t_edges, t_project,
                                t_tuples + t_nodes + t_edges + t_project, t_arrays]):
            steps[k].append(v)

    for k, v in steps.items():
        print(f"  {k:<13} {1000 * statistics.median(v):9.1f} ms")
    ratio = statistics.median(steps["vertex total"]) / statistics.median(steps["from_arrays"])
    print(f"  from_arrays is {ratio:.0f}x faster than the Vertex route")
    print(f"  projection memory: {direct.memory_usage() / len(ids):.0f} bytes/node "
          f"(through a Vertex: {via_vertex.memory_usage() / len(ids):.0f})")

    rng = np.random.default_rng(1)
    pairs = [(ids[a], ids[b]) for a, b in rng.integers(0, len(ids), (20, 2)).tolist()]
    assert direct.shortest_paths(pairs) == via_vertex.shortest_paths(pairs)
    assert direct.ids() == via_vertex.ids() and direct.edge_count() == via_vertex.edge_count()


if __name__ == "__main__":
    main()
