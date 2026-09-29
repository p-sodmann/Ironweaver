"""Vertex.shortest_paths / Vertex.distances: batches of queries run in
parallel on a compact snapshot, with the GIL released."""

import random
import threading
import time

import pytest

try:
    from ironweaver import Vertex
except Exception as e:  # pragma: no cover - module unavailable
    pytest.skip(f"ironweaver module unavailable: {e}", allow_module_level=True)


def diamond():
    #   a -> b -> d
    #   a -> c -> d      plus d -> e, and f unconnected
    g = Vertex()
    for n in "abcdef":
        g.add_node(n)
    g.add_edge("a", "b", {"weight": 1.0})
    g.add_edge("b", "d", {"weight": 5.0})
    g.add_edge("a", "c", {"weight": 2.0})
    g.add_edge("c", "d", {"weight": 1.0})
    g.add_edge("d", "e")
    return g


def random_graph(seed, n=300, m=1500):
    rng = random.Random(seed)
    g = Vertex()
    for i in range(n):
        g.add_node(f"n{i}")
    for _ in range(m):
        g.add_edge(f"n{rng.randrange(n)}", f"n{rng.randrange(n)}", {"weight": rng.uniform(0.1, 10)})
    return g


def path_cost(g, nodelist):
    """Cheapest edge weight along consecutive nodes (validates the path)."""
    total = 0.0
    for u, v in zip(nodelist, nodelist[1:]):
        weights = [e.attr.get("weight", 1.0) for e in g[u].edges if e.to_node.id == v]
        assert weights, f"no edge {u} -> {v}"
        total += min(weights)
    return total


def test_shortest_paths_basic():
    g = diamond()
    res = g.shortest_paths([("a", "e"), ("e", "a"), ("c", "c")], weight="weight")
    assert res[0] == {"nodelist": ["a", "c", "d", "e"], "cost": pytest.approx(4.0)}
    assert res[1] is None
    assert res[2] == {"nodelist": ["c"], "cost": 0.0}
    hops = g.shortest_paths([("a", "e")])[0]
    assert hops["cost"] == 3 and isinstance(hops["cost"], int) and len(hops["nodelist"]) == 4
    assert g.shortest_paths([("a", "e")], "bfs", max_cost=2) == [None]
    assert g.shortest_paths([("e", "a")], direction="in")[0]["cost"] == 3


def test_matches_single_queries_on_random_graphs():
    for seed in range(3):
        g = random_graph(seed)
        rng = random.Random(seed)
        pairs = [(f"n{rng.randrange(300)}", f"n{rng.randrange(300)}") for _ in range(60)]
        batch = g.shortest_paths(pairs, "dijkstra")
        hops = g.shortest_paths(pairs, "bfs", direction="both")
        for (s, t), r, h in zip(pairs, batch, hops):
            try:
                single = g.shortest_path(s, t, "dijkstra")
            except ValueError:
                assert r is None
            else:
                assert r["cost"] == pytest.approx(single.meta["cost"])
                assert r["nodelist"][0] == s and r["nodelist"][-1] == t
                assert path_cost(g, r["nodelist"]) == pytest.approx(r["cost"])
            try:
                single = g.shortest_path(s, t, "bfs", direction="both")
            except ValueError:
                assert h is None
            else:
                assert h["cost"] == single.meta["cost"]


def test_distances():
    g = diamond()
    d = g.distances(["a", "f"], weight="weight")
    assert d["a"] == pytest.approx({"a": 0.0, "b": 1.0, "c": 2.0, "d": 3.0, "e": 4.0})
    assert d["f"] == {"f": 0.0}
    assert g.distances(["a"], ["e", "b"])["a"] == {"b": 1, "e": 3}
    assert set(g.distances(["a"], weight="weight", max_cost=2)["a"]) == {"a", "b", "c"}
    assert set(g.distances(["e"], direction="in")["e"]) == {"a", "b", "c", "d", "e"}


def test_distances_match_shortest_paths():
    g = random_graph(7)
    sources = [f"n{i}" for i in range(0, 300, 37)]
    dist = g.distances(sources, method="dijkstra")
    for s in sources:
        targets = list(dist[s])[:20]
        paths = g.shortest_paths([(s, t) for t in targets], "dijkstra")
        for t, p in zip(targets, paths):
            assert p["cost"] == pytest.approx(dist[s][t])


def test_errors():
    g = diamond()
    with pytest.raises(ValueError, match="Root node"):
        g.shortest_paths([("zz", "a")])
    with pytest.raises(ValueError, match="Target node"):
        g.distances(["a"], ["zz"])
    with pytest.raises(ValueError, match="astar"):
        g.shortest_paths([("a", "e")], "astar")
    with pytest.raises(ValueError):
        g.shortest_paths([("a", "e")], "nope")
    with pytest.raises(ValueError):
        g.shortest_paths([("a", "e")], "bfs", weight="weight")
    with pytest.raises(ValueError):
        g.distances(["a"], max_cost=-1)
    # Weights are validated for the whole graph when the snapshot is built
    g.add_edge("f", "f", {"weight": -1})
    with pytest.raises(ValueError, match="non-negative"):
        g.shortest_paths([("a", "b")], "dijkstra")
    g.remove_edge("f", "f")
    g.add_edge("f", "a", {"weight": "heavy"})
    with pytest.raises(TypeError):
        g.distances(["a"], method="dijkstra", weight="weight")
    # ... but not for BFS, which ignores them
    assert g.shortest_paths([("a", "b")])[0]["cost"] == 1


def test_releases_the_gil():
    # A long batch on one thread must not stop another Python thread.
    g = Vertex()
    n = 20_000
    for i in range(n):
        g.add_node(str(i))
    rng = random.Random(1)
    for i in range(n):
        for _ in range(5):
            g.add_edge(str(i), str(rng.randrange(n)), {"weight": rng.random()})
    ticks = []
    stop = threading.Event()

    def ticker():
        while not stop.is_set():
            ticks.append(time.perf_counter())
            time.sleep(0.001)

    start = time.perf_counter()
    g.distances([str(i) for i in range(64)], method="dijkstra")  # warm up
    single = time.perf_counter() - start
    t = threading.Thread(target=ticker)
    t.start()
    start = time.perf_counter()
    g.distances([str(i) for i in range(64)], method="dijkstra")
    end = time.perf_counter()
    stop.set()
    t.join()
    during = [x for x in ticks if start <= x <= end]
    # Without the GIL released the ticker would be starved for the whole call
    assert len(during) >= 3 or single < 0.02
