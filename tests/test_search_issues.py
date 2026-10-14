"""Regressions for search statistics, source identity and weighted BFS."""
import gc
import math
import random

import pytest

from ironweaver import Vertex
from test_pathfinding import grid


@pytest.mark.parametrize("method", ["bfs", "dijkstra", "astar"])
def test_stats_preserve_results_and_expose_core_counts(method):
    g = grid(8)
    p = g.project(weight="weight", coords=["x", "y"])
    pairs = [("0,0", "7,7"), ("2,2", "2,2")]
    kw = {"heuristic": "manhattan"} if method == "astar" else {}
    for obj in (g, p):
        plain = obj.shortest_paths(pairs, method, **kw)
        measured = obj.shortest_paths(pairs, method, stats=True, **kw)
        assert [{k: v for k, v in r.items() if k != "stats"} for r in measured] == plain
        assert measured[1]["stats"] == {"settled": 1}
        assert all(r["stats"]["settled"] > 0 for r in measured)
        # Exact core counts for this grid: A* follows 15 tiles; Dijkstra settles all 64.
        if method != "bfs":
            assert measured[0]["stats"] == {"settled": 15 if method == "astar" else 64}
    plain = g.shortest_path(*pairs[0], method, **kw)
    measured = g.shortest_path(*pairs[0], method, stats=True, **kw)
    assert "stats" not in plain.meta
    assert measured.meta["nodelist"] == plain.meta["nodelist"]
    if method != "bfs":
        assert measured.meta["stats"]["settled"] == measured.meta["expanded"]


def test_stats_astar_settles_fewer_nodes_and_unreachable_stays_none():
    g = grid(20)
    g.add_node("isolated", {"x": -1, "y": -1})
    p = g.project(weight="weight", coords=["x", "y"])
    pairs = [("0,0", "19,19"), ("0,0", "isolated")]
    d = p.shortest_paths(pairs, "dijkstra", stats=True)
    a = p.shortest_paths(pairs, "astar", heuristic="manhattan", stats=True)
    assert a[0]["stats"]["settled"] <= d[0]["stats"]["settled"]
    assert a[0]["cost"] == d[0]["cost"]
    assert a[1] is d[1] is None


@pytest.mark.parametrize("direction", ["out", "in", "both"])
def test_weighted_bfs_matches_projection_randomized(direction):
    rng = random.Random(121)
    for _ in range(12):
        g = Vertex()
        ids = [str(i) for i in range(30)]
        g.add_nodes(ids)
        for _ in range(140):
            a, b = rng.choices(ids, k=2)
            weight = rng.choice([0, 1, 7, math.inf, None])
            g.add_edge(a, b, {} if weight is None else {"w": weight})
        p = g.project(weight="w", direction=direction)
        pairs = [tuple(rng.choices(ids, k=2)) for _ in range(20)]
        expected = p.shortest_paths(pairs, "bfs")
        assert g.shortest_paths(pairs, "bfs", weight="w", direction=direction) == expected
        for (a, b), want in zip(pairs, expected):
            if want is None:
                with pytest.raises(ValueError, match="not reachable"):
                    g.shortest_path(a, b, "bfs", weight="w", direction=direction)
            else:
                got = g.shortest_path(a, b, "bfs", weight="w", direction=direction).meta
                assert got["nodelist"] == want["nodelist"]
                assert got["cost"] == want["cost"]


def test_bfs_weight_defaults_validation_and_unweighted_node_search():
    g = Vertex()
    g.add_nodes(["a", "b", "c"])
    g.add_edge("a", "b", {"w": math.inf})
    g.add_edge("a", "c")
    g.add_edge("c", "b")
    assert g.shortest_path("a", "b", "bfs", weight="w").meta["nodelist"] == ["a", "c", "b"]
    assert g.shortest_path("a", "b", "bfs").meta["nodelist"] == ["a", "b"]
    assert g["a"].bfs_search("b", depth=1) == g["b"]
    with pytest.raises(ValueError, match="not reachable"):
        g.shortest_path("a", "b", "bfs", weight="w", default_weight=math.inf)
    for invalid in (-1, math.nan, "invalid"):
        g.get_node("a").edges[0].attr_set("w", invalid)
        with pytest.raises((ValueError, TypeError)):
            g.shortest_path("a", "b", "bfs", weight="w")


def test_bfs_search_does_not_revisit_edges_for_canonical_path():
    g = Vertex()
    g.add_nodes(["a", "b", "c", "d"])
    g.add_edges([("a", "b"), ("a", "c"), ("b", "d"), ("c", "d")])
    seen = set()
    def once(edge):
        assert edge.id not in seen
        seen.add(edge.id)
        return True
    assert g["a"].bfs_search("d", filter=once) == g["d"]
    assert g["a"].bfs_search("d", depth=1) is None
    assert g["a"].bfs_search("a", depth=0) == g["a"]


def test_projection_rejects_same_slot_after_source_collection():
    source = Vertex()
    source.add_nodes(["a", "b"])
    edge = source.add_edge("a", "b", {"weight": 1})
    p = source.project(weight="weight")
    assert p.edge_index(edge) == 0
    original_address = id(source)
    del edge, source
    gc.collect()
    # Every replacement uses the same EdgeIx. CPython often also reuses the
    # Vertex's address immediately; rejection must hold in either case.
    for _ in range(2000):
        replacement = Vertex()
        replacement.add_nodes(["a", "b"])
        edge = replacement.add_edge("a", "b", {"weight": 2})
        with pytest.raises(ValueError, match="another Vertex"):
            p.edge_index(edge)
        reused = id(replacement) == original_address
        del edge, replacement
        if reused:
            break
    assert p.shortest_paths([("a", "b")])[0]["cost"] == 1
    if not reused:
        pytest.skip("allocator did not reuse the Vertex address after 2000 attempts")
