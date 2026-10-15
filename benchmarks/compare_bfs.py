"""Time canonical BFS paths and reachability on a grid and sparse random graph.

Run the same command on the base and PR revisions (release build):
    RAYON_NUM_THREADS=1 python benchmarks/compare_bfs.py
"""
import argparse
import gc
import random
import statistics
import time

from ironweaver import Vertex


def graphs(side, seed):
    grid = Vertex()
    grid.add_nodes([str(i) for i in range(side * side)])
    edges = []
    for y in range(side):
        for x in range(side):
            u = y * side + x
            for v in ([u + 1] if x + 1 < side else []) + ([u + side] if y + 1 < side else []):
                edges.extend([(str(u), str(v)), (str(v), str(u))])
    grid.add_edges(edges)
    rng = random.Random(seed)
    n = side * side
    sparse = Vertex()
    sparse.add_nodes([str(i) for i in range(n)])
    # Connected, with three extra outgoing edges per node.
    edges = [(str(i), str((i + 1) % n)) for i in range(n)]
    edges += [(str(i), str(rng.randrange(n))) for i in range(n) for _ in range(3)]
    sparse.add_edges(edges)
    pairs = [(str(rng.randrange(n)), str(rng.randrange(n))) for _ in range(100)]
    corners = ["0", str(side - 1), str(n - side), str(n - 1)]
    corner_pairs = [(corners[i], corners[3 - i]) for i in range(4)] * 25
    return [("grid random trips", grid, pairs), ("grid corner trips", grid, corner_pairs), ("random", sparse, pairs)]


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--side", type=int, default=100)
    ap.add_argument("--repeats", type=int, default=7)
    ap.add_argument("--seed", type=int, default=120)
    args = ap.parse_args()
    for name, graph, pairs in graphs(args.side, args.seed):
        sources = [graph.get_node(a) for a, _ in pairs]
        calls = {
            "shortest_path": lambda: [graph.shortest_path(a, b, "bfs") for a, b in pairs],
            "bfs_search": lambda: [a.bfs_search(b) for a, (_, b) in zip(sources, pairs)],
        }
        print(f"{name}: {graph.node_count()} nodes, {len(pairs)} queries")
        for method, call in calls.items():
            call()
            samples = []
            for _ in range(args.repeats):
                gc.collect()
                start = time.perf_counter()
                call()
                samples.append((time.perf_counter() - start) * 1000 / len(pairs))
            print(f"  {method}: {statistics.median(samples):.4f} ms/query")


if __name__ == "__main__":
    main()
