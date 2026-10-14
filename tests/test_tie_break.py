"""Equal-cost paths: the choice is fixed by node order alone.

Walking back from the target, every path method steps to the earliest node
(in node order: insertion order for a Vertex, the index for a Projection)
through which a cheapest route arrives. The result must not depend on the
order edges were added, the process (hash seeds) or the number of threads.
"""
import json
import os
import random
import subprocess
import sys

import pytest

from ironweaver import Projection, Vertex

W, H = 7, 5
IDS = [f"{x},{y}" for y in range(H) for x in range(W)]
PAIRS = [("0,0", "6,4"), ("6,4", "0,0"), ("6,0", "0,4"), ("3,2", "0,0"), ("1,3", "5,1"), ("0,2", "6,2")]


def tile_cost(i):
    return 2.0 if i % 5 == 3 else 1.0


def grid_edges(unit):
    """Edges both ways between neighbours; entering tile i costs tile_cost(i)."""
    edges = []
    for y in range(H):
        for x in range(W):
            u = y * W + x
            for nx, ny in ((x + 1, y), (x, y + 1)):
                if nx < W and ny < H:
                    v = ny * W + nx
                    edges.append((u, v, 1.0 if unit else tile_cost(v)))
                    edges.append((v, u, 1.0 if unit else tile_cost(u)))
    return edges


def grid(edges):
    v = Vertex()
    for i, node_id in enumerate(IDS):
        v.add_node(node_id, {"x": i % W, "y": i // W})
    for a, b, w in edges:
        v.add_edge(IDS[a], IDS[b], {"weight": w})
    return v


def reference(edges, source, target, unit):
    """Exact distances, then from the target back to the lowest-indexed
    predecessor on a cheapest route."""
    arcs = [(a, b, 1.0 if unit else w) for a, b, w in edges]
    dist = [float("inf")] * len(IDS)
    dist[IDS.index(source)] = 0.0
    changed = True
    while changed:
        changed = False
        for a, b, w in arcs:
            if dist[a] + w < dist[b]:
                dist[b] = dist[a] + w
                changed = True
    path = [IDS.index(target)]
    while path[-1] != IDS.index(source):
        v = path[-1]
        path.append(min(a for a, b, w in arcs if b == v and dist[a] + w == dist[v]))
    return [IDS[i] for i in reversed(path)]


def results(edge_seed):
    """Every path method on the grid with the edges in a shuffled order."""
    out = {}
    for unit in (True, False):
        edges = grid_edges(unit)
        random.Random(edge_seed).shuffle(edges)
        v = grid(edges)
        key = "unit" if unit else "weighted"
        for method in ("bfs", "dijkstra", "astar"):
            kw = {"heuristic": "manhattan"} if method == "astar" else {}
            for direction in ("out", "in", "both"):
                out[f"{key} single {method} {direction}"] = [
                    v.shortest_path(s, t, method, direction=direction, **kw).meta["nodelist"] for s, t in PAIRS
                ]
            weight = {} if method == "bfs" else {"weight": "weight"}
            batch = v.shortest_paths(PAIRS, method, **weight, **kw)
            out[f"{key} vertex batch {method}"] = [r["nodelist"] for r in batch]
        p = v.project(weight="weight", coords=["x", "y"])
        arrays = Projection.from_arrays(
            IDS,
            [a for a, _, _ in edges],
            [b for _, b, _ in edges],
            [w for _, _, w in edges],
            [(i % W, i // W) for i in range(len(IDS))],
        )
        for name, proj in (("project", p), ("from_arrays", arrays)):
            for method in ("bfs", "dijkstra", "astar"):
                kw = {"heuristic": "manhattan"} if method == "astar" else {}
                out[f"{key} {name} {method}"] = [r["nodelist"] for r in proj.shortest_paths(PAIRS, method, **kw)]
            for method in ("bfs", "dijkstra"):
                out[f"{key} {name} ksp {method}"] = [
                    [r["nodelist"] for r in proj.k_shortest_paths(s, t, 3, method)] for s, t in PAIRS
                ]
    return out


@pytest.mark.parametrize("unit", [True, False])
def test_paths_follow_the_rule(unit):
    edges = grid_edges(unit)
    random.Random(5).shuffle(edges)
    v = grid(edges)
    p = v.project(weight="weight", coords=["x", "y"])
    method = "bfs" if unit else "dijkstra"
    want = [reference(edges, s, t, unit) for s, t in PAIRS]
    weight = {} if unit else {"weight": "weight"}
    assert [v.shortest_path(s, t, method, **weight).meta["nodelist"] for s, t in PAIRS] == want
    assert [r["nodelist"] for r in v.shortest_paths(PAIRS, method, **weight)] == want
    assert [r["nodelist"] for r in p.shortest_paths(PAIRS, method)] == want
    assert [p.k_shortest_paths(s, t, 1, method)[0]["nodelist"] for s, t in PAIRS] == want
    # Hop counts on the weighted grid follow the rule too
    hops = [reference(edges, s, t, True) for s, t in PAIRS]
    assert [r["nodelist"] for r in p.shortest_paths(PAIRS, "bfs")] == hops
    assert [v.shortest_path(s, t, "bfs").meta["nodelist"] for s, t in PAIRS] == hops


def test_node_order_decides_edge_order_does_not():
    # a -> b -> d and a -> c -> d cost the same: the earlier node wins
    for edges in ([("a", "b"), ("b", "d"), ("a", "c"), ("c", "d")], [("c", "d"), ("a", "c"), ("b", "d"), ("a", "b")]):
        for nodes, want in (("abcd", ["a", "b", "d"]), ("acbd", ["a", "c", "d"])):
            v = Vertex()
            for n in nodes:
                v.add_node(n)
            for s, t in edges:
                v.add_edge(s, t, {"weight": 1.0})
            for method in ("bfs", "dijkstra"):
                assert v.shortest_path("a", "d", method).meta["nodelist"] == want
                assert v.shortest_paths([("a", "d")], method)[0]["nodelist"] == want


def test_same_paths_across_processes_threads_and_edge_orders():
    expected = results(0)
    code = "import json, sys; from test_tie_break import results; print(json.dumps(results(int(sys.argv[1]))))"
    here = os.path.dirname(os.path.abspath(__file__))
    for hash_seed, threads, edge_seed in [("0", "1", 0), ("123", "4", 1), ("random", "2", 2), ("7", "3", 3)]:
        env = dict(os.environ, PYTHONHASHSEED=hash_seed, RAYON_NUM_THREADS=threads)
        env["PYTHONPATH"] = here + os.pathsep + env.get("PYTHONPATH", "")
        proc = subprocess.run(
            [sys.executable, "-c", code, str(edge_seed)], env=env, capture_output=True, text=True, check=True
        )
        assert json.loads(proc.stdout) == expected, (hash_seed, threads, edge_seed)
