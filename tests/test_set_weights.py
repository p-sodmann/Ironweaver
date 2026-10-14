"""Projection.set_weights: weights changed in place give the same results,
for every method, as a projection built from scratch with the new weights;
an infinite weight makes an edge impassable."""

import math
import random
import threading

import pytest

from ironweaver import Projection, Vertex, attr

np = pytest.importorskip("numpy")

from test_from_arrays import random_arrays, results, vertex_from  # noqa: E402


def with_infinities(weight, rng, share=0.15):
    return [math.inf if rng.random() < share else w for w in weight]


def full_results(p):
    """`results` plus A* (the projections here have coordinates)."""
    out = results(p)
    ids = p.ids()
    rng = random.Random(11)
    pairs = [(rng.choice(ids), rng.choice(ids)) for _ in range(40)]
    out["astar"] = p.shortest_paths(pairs, "astar", heuristic="manhattan", heuristic_scale=0.5)
    out["distances_bfs"] = p.distances(ids[:10], method="bfs", max_cost=3)
    out["closeness_hops"] = p.closeness_centrality(weighted=False)
    out["mst_max"] = p.minimum_spanning_tree(maximum=True)
    out["ksp_bfs"] = p.k_shortest_paths(pairs[2][0], pairs[3][1], 3, "bfs")
    return out


@pytest.mark.parametrize("direction", ["out", "in", "both"])
@pytest.mark.parametrize("seed", [1, 2, 3])
def test_same_results_as_a_rebuild(direction, seed):
    rng = random.Random(seed)
    ids, src, dst, weight = random_arrays(seed, n=80, m=400)
    weight = with_infinities(weight, rng)
    coords = [[float(i % 9), float(i // 9)] for i in range(len(ids))]
    p = Projection.from_arrays(ids, src, dst, weight, coords, direction=direction)
    if seed % 2:
        p.neighbors(ids[0], "in")  # the transposed adjacency exists already
    for round_ in range(3):
        k = rng.randrange(1, 60)
        edges = [rng.randrange(len(src)) for _ in range(k)]
        new = [rng.choice([math.inf, 0.0, float(rng.randint(1, 9))]) for _ in range(k)]
        for e, w in zip(edges, new):
            weight[e] = w  # the last one wins for repeated edges
        if round_ == 0:
            p.set_weights(edges, new)
        elif round_ == 1:
            p.set_weights(np.array(edges, dtype=np.int32), np.array(new, dtype=np.float32))
        else:
            p.set_weights(np.array(edges), np.array(new))
    rebuilt = Projection.from_arrays(ids, src, dst, weight, coords, direction=direction)
    assert full_results(p) == full_results(rebuilt)


@pytest.mark.parametrize("direction", ["out", "in", "both"])
def test_vertex_projection_edge_order(direction):
    ids, src, dst, weight = random_arrays(4, n=60, m=300)
    g = vertex_from(ids, src, dst, weight)
    # Remove a few edges and add new ones (which reuse the removed slots), and
    # leave out heavy edges: the projected order is the graph's edge order
    # among the kept edges
    edges = [e for i in ids for e in g[i].edges]
    for e in edges[:10:3]:
        g.remove_edge(e.from_node.id, e.to_node.id)
    g.add_edge(ids[0], ids[1], {"weight": 2.0})
    g.add_edge(ids[1], ids[1], {"weight": 3.0})
    p = g.project(weight="weight", direction=direction, edge_filter=attr("weight") != 4.0)
    kept = [e for i in ids for e in g[i].edges if e.attr["weight"] != 4.0]
    order = sorted(kept, key=p.edge_index)
    assert sorted(p.edge_index(e) for e in kept) == list(range(p.edge_count()))

    rng = random.Random(5)
    changed = rng.sample(order, 25)
    new = [rng.choice([math.inf, 0.5, 7.0]) for _ in changed]
    for e, w in zip(changed, new):
        e.attr_set("weight", w)
    p.set_weights([p.edge_index(e) for e in changed], new)
    again = g.project(weight="weight", direction=direction, edge_filter=attr("weight") != 4.0)
    assert results(p) == results(again)

    with pytest.raises(ValueError, match="not in the projection"):
        p.edge_index(next(e for i in ids for e in g[i].edges if e.attr["weight"] == 4.0))
    other = Vertex()
    other.add_node(ids[0]); other.add_node(ids[1]); other.add_edge(ids[0], ids[1], {"weight": 1.0})
    with pytest.raises(ValueError, match="another Vertex"):
        p.edge_index(other[ids[0]].edges[0])
    with pytest.raises(ValueError, match="weighted projection"):
        g.project().edge_index(kept[0])
    with pytest.raises(ValueError, match="weighted projection"):
        Projection.from_arrays(ids, src, dst, weight).edge_index(kept[0])


def test_a_building_blocks_a_tile():
    """The game case: a grid whose tiles get blocked and unblocked."""
    w, h = 30, 20
    ids = [f"{x},{y}" for y in range(h) for x in range(w)]
    src, dst = [], []
    for y in range(h):
        for x in range(w):
            for dx, dy in [(1, 0), (0, 1)]:
                if x + dx < w and y + dy < h:
                    a, b = y * w + x, (y + dy) * w + x + dx
                    src += [a, b]
                    dst += [b, a]
    src, dst = np.array(src), np.array(dst)
    cost = np.ones(len(src))
    coords = np.array([[x, y] for y in range(h) for x in range(w)], dtype=float)
    p = Projection.from_arrays(ids, src, dst, cost, coords)
    # Block a wall at x = 10 except at y = 19: every edge into its tiles
    wall = [y * w + 10 for y in range(h - 1)]
    into_wall = np.flatnonzero(np.isin(dst, wall))
    p.set_weights(into_wall, np.full(len(into_wall), np.inf))
    for method in ["dijkstra", "astar", "bfs"]:
        [path] = p.shortest_paths([("5,5", "15,5")], method)
        assert all(not n.startswith("10,") or n == "10,19" for n in path["nodelist"]), method
        assert path["cost"] == 10 + 2 * 14
    assert "10,5" not in p.distances(["5,5"])["5,5"]
    assert "10,5" not in p.bfs_levels(["5,5"])
    # A road: cheaper tiles along y = 19
    road = np.flatnonzero(np.isin(dst, [19 * w + x for x in range(w)]))
    p.set_weights(road, np.full(len(road), 0.5))
    rebuilt_cost = cost.copy()
    rebuilt_cost[into_wall] = np.inf
    rebuilt_cost[road] = 0.5
    q = Projection.from_arrays(ids, src, dst, rebuilt_cost, coords)
    pairs = [("5,5", "15,5"), ("0,0", "29,19"), ("10,19", "10,0")]
    for method in ["dijkstra", "astar", "bfs"]:
        assert p.shortest_paths(pairs, method) == q.shortest_paths(pairs, method)
    # Demolished: the straight path is back
    p.set_weights(into_wall, np.ones(len(into_wall)))
    [path] = p.shortest_paths([("5,5", "15,5")])
    assert path["cost"] == 10 and "10,5" in path["nodelist"]


def test_infinite_edges_match_networkx_without_them():
    nx = pytest.importorskip("networkx")
    ids, src, dst, weight = random_arrays(6, n=60, m=300)
    weight = with_infinities(weight, random.Random(6), share=0.3)
    p = Projection.from_arrays(ids, src, dst, weight)
    G = nx.MultiDiGraph()
    G.add_nodes_from(ids)
    for s, t, w in zip(src, dst, weight):
        if w != math.inf:
            G.add_edge(ids[s], ids[t], weight=w)
    dist = p.distances(ids[:5])
    hops = p.distances(ids[:5], method="bfs")
    for source in ids[:5]:
        want = nx.single_source_dijkstra_path_length(G, source)
        assert dist[source] == pytest.approx(want)
        assert hops[source] == nx.single_source_shortest_path_length(G, source)
    simple = nx.DiGraph()
    simple.add_nodes_from(ids)
    for a, b, d in G.edges(data=True):
        if a != b and (not simple.has_edge(a, b) or simple[a][b]["weight"] > d["weight"]):
            simple.add_edge(a, b, weight=d["weight"])
    got = p.betweenness_centrality(normalized=False, weighted=False)
    want = nx.betweenness_centrality(simple, normalized=False)
    assert got == pytest.approx(want)


def test_set_weights_errors():
    ids, src, dst = ["a", "b", "c"], [0, 1], [1, 2]
    p = Projection.from_arrays(ids, src, dst, [1.0, 2.0])
    with pytest.raises(ValueError, match="same length, got 2 and 1"):
        p.set_weights([0, 1], [1.0])
    with pytest.raises(ValueError, match=r"edges\[1\] is 2, not an edge index \(0 to 1\)"):
        p.set_weights(np.array([0, 2]), np.ones(2))
    with pytest.raises(ValueError, match=r"edges\[0\] is -1"):
        p.set_weights([-1], [1.0])
    with pytest.raises(ValueError, match=r"non-negative, got -1 \(weights\[0\]\)"):
        p.set_weights([0], [-1.0])
    with pytest.raises(ValueError, match=r"non-negative, got NaN \(weights\[1\]\)"):
        p.set_weights([0, 1], np.array([1.0, np.nan]))
    with pytest.raises(TypeError, match="edges must be an integer array"):
        p.set_weights(np.array([0.0]), [1.0])
    with pytest.raises(TypeError, match="weights must be a float array"):
        p.set_weights([0], ["x"])
    with pytest.raises(ValueError, match="1 dimension, got 2"):
        p.set_weights(np.zeros((1, 1), dtype=np.int64), [1.0])
    # Nothing was changed by the failed calls
    assert p.shortest_paths([("a", "c")])[0]["cost"] == 3.0
    p.set_weights([], [])
    unweighted = Projection.from_arrays(ids, src, dst)
    with pytest.raises(ValueError, match="needs a weighted projection"):
        unweighted.set_weights([0], [1.0])
    g = Vertex()
    g.add_node("a"); g.add_node("b"); g.add_edge("a", "b")
    with pytest.raises(ValueError, match="needs a weighted projection"):
        g.project().set_weights([0], [1.0])


def test_no_change_while_a_query_runs():
    # A query holds the projection (with the GIL released); set_weights in
    # another thread meanwhile raises instead of changing weights under it
    n = 150
    ids = [str(i) for i in range(n * n)]
    right = [(y * n + x, y * n + x + 1) for y in range(n) for x in range(n - 1)]
    down = [(y * n + x, (y + 1) * n + x) for y in range(n - 1) for x in range(n)]
    edges = np.array(right + down)
    p = Projection.from_arrays(ids, edges[:, 0], edges[:, 1], np.ones(len(edges)), direction="both")
    refused = []
    for _ in range(5):
        worker = threading.Thread(target=lambda: p.distances(ids[:: n * 4]))
        worker.start()
        while worker.is_alive():
            try:
                p.set_weights([0], [2.0])
            except RuntimeError as e:
                refused.append(str(e))
                break
        worker.join()
        if refused:
            break
    assert refused and "in use" in refused[0]
    # Once it is done, weights change again
    p.set_weights([0], [3.0])
    assert p.shortest_paths([("0", "1")])[0]["cost"] == 3.0
