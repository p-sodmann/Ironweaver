"""Vertex.shortest_path(..., method=...): method selection, options, and A*."""

import math
import random

import pytest

from ironweaver import Vertex

nx = pytest.importorskip("networkx")


def grid(n, **extra):
    """n x n grid, edges both ways between neighbours, weight 1, x/y coords."""
    g = Vertex()
    for x in range(n):
        for y in range(n):
            g.add_node(f"{x},{y}", {"x": x, "y": y, "pos": (x, y), "geo": {"lat": x, "lon": y}, **extra})
    for x in range(n):
        for y in range(n):
            for dx, dy in ((1, 0), (0, 1), (-1, 0), (0, -1)):
                if 0 <= x + dx < n and 0 <= y + dy < n:
                    g.add_edge(f"{x},{y}", f"{x + dx},{y + dy}", {"weight": 1.0})
    return g


def geometric(seed, n=60, k=4):
    """Random points; edges to nearby points, weight >= Euclidean distance so
    the Euclidean heuristic is admissible. Returns (ironweaver, networkx)."""
    rng = random.Random(seed)
    pts = {f"p{i}": (rng.uniform(0, 100), rng.uniform(0, 100)) for i in range(n)}
    g, h = Vertex(), nx.MultiDiGraph()
    for i, (x, y) in pts.items():
        g.add_node(i, {"x": x, "y": y, "pos": [x, y], "loc": {"xy": {"x": x, "y": y}}})
        h.add_node(i, x=x, y=y)
    for i, (x, y) in pts.items():
        near = sorted(pts, key=lambda j: math.dist(pts[i], pts[j]))[1 : k + 1]
        for j in near + rng.sample(list(pts), 1):
            if j == i:
                continue
            w = math.dist(pts[i], pts[j]) * rng.uniform(1.0, 1.5)
            g.add_edge(i, j, {"weight": w})
            h.add_edge(i, j, weight=w)
    return g, h


def euclid(h):
    return lambda u, v: math.dist((h.nodes[u]["x"], h.nodes[u]["y"]), (h.nodes[v]["x"], h.nodes[v]["y"]))


# --- the interface ------------------------------------------------------------

def test_path_methods_lists_all_methods():
    methods = Vertex.path_methods()
    assert list(methods) == ["bfs", "dijkstra", "astar"]
    assert all(isinstance(d, str) and d for d in methods.values())


@pytest.mark.parametrize("kwargs, method", [
    ({}, "bfs"),
    ({"max_depth": 20}, "bfs"),
    ({"weight": "weight"}, "dijkstra"),
    ({"heuristic": "manhattan"}, "astar"),
    ({"coords": "pos"}, "astar"),
    ({"weight": "weight", "coords": ["x", "y"]}, "astar"),
])
def test_method_is_picked_automatically(kwargs, method):
    g = grid(4)
    if "distances" in kwargs:
        g.meta["d"] = {}
    assert g.shortest_path("0,0", "3,3", **kwargs).meta["method"] == method


def test_distances_option_picks_astar():
    g = grid(3)
    g.meta["d"] = {}
    assert g.shortest_path("0,0", "2,2", distances="d").meta["method"] == "astar"


def test_result_meta_per_method():
    g = grid(4)
    bfs = g.shortest_path("0,0", "3,3", method="bfs").meta
    assert bfs["cost"] == 6 and len(bfs["nodelist"]) == 7 and "expanded" not in bfs
    for method in ("dijkstra", "astar"):
        meta = g.shortest_path("0,0", "3,3", method=method).meta
        assert meta["method"] == method
        assert meta["cost"] == 6.0
        assert meta["nodelist"][0] == "0,0" and meta["nodelist"][-1] == "3,3"
        assert meta["expanded"] >= 1


def test_result_contains_path_nodes_and_edges():
    g = grid(3)
    p = g.shortest_path("0,0", "2,2", method="astar")
    assert sorted(p.keys()) == sorted(p.meta["nodelist"])
    assert p["0,0"] is not g["0,0"]  # copies, like the other path methods


def test_shorthands_match_shortest_path():
    g, _ = geometric(3)
    a = g.shortest_path_dijkstra("p0", "p1")
    b = g.shortest_path("p0", "p1", method="dijkstra")
    assert a.meta["nodelist"] == b.meta["nodelist"] and a.meta["cost"] == b.meta["cost"]
    a = g.shortest_path_bfs("p0", "p1", max_depth=50)
    b = g.shortest_path("p0", "p1", method="bfs", max_depth=50)
    assert len(a.meta["nodelist"]) == len(b.meta["nodelist"])


def test_source_equals_target():
    g = grid(2)
    for method in ("bfs", "dijkstra", "astar"):
        p = g.shortest_path("1,1", "1,1", method=method)
        assert p.meta["nodelist"] == ["1,1"] and p.meta["cost"] == 0


@pytest.mark.parametrize("kwargs, error, text", [
    ({"method": "nope"}, ValueError, "unknown path method 'nope'"),
    ({"method": "bfs", "heuristic": "euclidean"}, TypeError, "does not accept option 'heuristic'"),
    ({"method": "dijkstra", "max_depth": 3}, TypeError, "accepted options: none"),
    ({"method": "astar", "max_depth": 3}, TypeError, "heuristic, coords, distances"),
    ({"method": "bfs", "weight": "weight"}, ValueError, "ignores edge weights"),
    ({"method": "dijkstra", "max_cost": -1}, ValueError, "max_cost must be non-negative"),
    ({"direction": "sideways"}, ValueError, "direction must be"),
])
def test_invalid_arguments(kwargs, error, text):
    with pytest.raises(error, match=text):
        grid(2).shortest_path("0,0", "1,1", **kwargs)


def test_missing_nodes_and_unreachable():
    g = grid(2)
    g.add_node("island", {"x": 9, "y": 9})
    for method in ("bfs", "dijkstra", "astar"):
        with pytest.raises(ValueError, match="not found"):
            g.shortest_path("nope", "0,0", method=method)
        with pytest.raises(ValueError, match="not found"):
            g.shortest_path("0,0", "nope", method=method)
        with pytest.raises(ValueError, match="not reachable"):
            g.shortest_path("0,0", "island", method=method)


def test_none_options_are_ignored():
    g = grid(3)
    assert g.shortest_path("0,0", "2,2", method="dijkstra", max_depth=None).meta["method"] == "dijkstra"


# --- A* correctness -----------------------------------------------------------

@pytest.mark.parametrize("seed", range(30))
def test_astar_matches_networkx_and_dijkstra(seed):
    g, h = geometric(seed)
    rng = random.Random(seed)
    for _ in range(3):
        s, t = rng.sample(list(h), 2)
        try:
            expected = nx.astar_path_length(h, s, t, heuristic=euclid(h), weight="weight")
        except nx.NetworkXNoPath:
            with pytest.raises(ValueError, match="not reachable"):
                g.shortest_path(s, t, method="astar")
            continue
        dijkstra = g.shortest_path(s, t, method="dijkstra").meta
        assert dijkstra["cost"] == pytest.approx(expected)
        for coords in (None, ["x", "y"], "pos", ["loc.xy.x", "loc.xy.y"]):
            meta = g.shortest_path(s, t, method="astar", coords=coords).meta
            assert meta["cost"] == pytest.approx(expected)
            assert meta["expanded"] <= dijkstra["expanded"]
            # the path is real and its edges add up to the cost
            nodes = meta["nodelist"]
            total = sum(min(d["weight"] for d in h.get_edge_data(u, v).values()) for u, v in zip(nodes, nodes[1:]))
            assert total == pytest.approx(expected)


@pytest.mark.parametrize("direction", ["out", "in", "both"])
def test_astar_directions(direction):
    g, h = geometric(7)
    if direction == "both":
        # to_undirected() would merge u->v and v->u into one edge; keep both
        view = nx.MultiGraph()
        view.add_edges_from(h.edges(data=True))
    else:
        view = h if direction == "out" else h.reverse(copy=False)
    for t in ("p1", "p2", "p3"):
        try:
            expected = nx.dijkstra_path_length(view, "p0", t, weight="weight")
        except nx.NetworkXNoPath:
            continue
        meta = g.shortest_path("p0", t, method="astar", direction=direction).meta
        assert meta["cost"] == pytest.approx(expected)


def test_astar_expands_fewer_nodes_than_dijkstra_on_a_grid():
    g = grid(15)
    dijkstra = g.shortest_path("0,0", "14,14", method="dijkstra").meta
    for heuristic in ("euclidean", "manhattan"):
        astar = g.shortest_path("0,0", "14,14", heuristic=heuristic).meta
        assert astar["cost"] == dijkstra["cost"] == 28.0
        assert astar["expanded"] < dijkstra["expanded"]
    assert g.shortest_path("0,0", "14,14", heuristic="manhattan").meta["expanded"] == 29


def test_astar_respects_max_cost_and_weights():
    g = grid(4)
    assert g.shortest_path("0,0", "3,3", method="astar", max_cost=6).meta["cost"] == 6.0
    with pytest.raises(ValueError, match="not reachable"):
        g.shortest_path("0,0", "3,3", method="astar", max_cost=5.5)
    p = g.shortest_path("0,0", "3,3", method="astar", weight="missing", default_weight=2.0)
    assert p.meta["cost"] == 12.0


def test_astar_nodes_without_coordinates_count_as_zero():
    g = grid(4)
    g["1,0"].attr = {}  # no coordinates: estimate 0, still found
    g["2,2"].attr = {"x": 2}  # partial coordinates count as missing
    assert g.shortest_path("0,0", "3,3", method="astar").meta["cost"] == 6.0


def test_astar_three_dimensional_coords():
    g = Vertex()
    for i, p in enumerate([(0, 0, 0), (1, 1, 1), (2, 2, 2), (0, 0, 5)]):
        g.add_node(str(i), {"p": p})
    g.add_edge("0", "1", {"weight": 2})
    g.add_edge("1", "2", {"weight": 2})
    g.add_edge("0", "3", {"weight": 1})
    g.add_edge("3", "2", {"weight": 10})
    assert g.shortest_path("0", "2", coords="p").meta["nodelist"] == ["0", "1", "2"]


# --- A* with precomputed distances ---------------------------------------------

def test_distances_single_target_table():
    g = grid(10)
    g.meta["to_corner"] = {n: abs(9 - g[n].attr_get("x")) + abs(9 - g[n].attr_get("y")) for n in g.keys()}
    meta = g.shortest_path("0,0", "9,9", distances="to_corner").meta
    assert meta["cost"] == 18.0 and meta["expanded"] == 19


def test_distances_per_target_table_and_missing_entries():
    g = grid(10)
    targets = ["9,9", "0,9"]
    table = {}
    for n in g.keys():
        x, y = g[n].attr_get("x"), g[n].attr_get("y")
        table[n] = {t: abs(int(t[0]) - x) + abs(int(t[2]) - y) for t in targets}
    del table["5,5"]            # missing node entry -> 0
    del table["4,4"]["9,9"]     # missing target entry -> 0
    g.meta["h"] = table
    dijkstra = g.shortest_path("0,0", "9,9", method="dijkstra").meta
    for t in targets:
        meta = g.shortest_path("0,0", t, distances="h").meta
        assert meta["cost"] == g.shortest_path("0,0", t, method="dijkstra").meta["cost"]
    assert g.shortest_path("0,0", "9,9", distances="h").meta["expanded"] < dijkstra["expanded"]
    # a target not in the table: every estimate is 0, i.e. Dijkstra
    assert g.shortest_path("0,0", "5,0", distances="h").meta["cost"] == 5.0


def test_distances_is_read_live_from_meta():
    g = grid(3)
    g.meta["d"] = {}
    first = g.shortest_path("0,0", "2,2", distances="d").meta["expanded"]
    g.meta["d"] = {n: abs(2 - g[n].attr_get("x")) + abs(2 - g[n].attr_get("y")) for n in g.keys()}
    assert g.shortest_path("0,0", "2,2", distances="d").meta["expanded"] < first


# --- A* errors ------------------------------------------------------------------

def test_astar_errors():
    g = grid(3)
    g.meta["bad"] = {"0,0": "far"}
    g.meta["notdict"] = [1, 2]
    g.add_node("nowhere")
    g.add_edge("2,2", "nowhere")
    cases = [
        ({"distances": "missing"}, ValueError, "no key 'missing'"),
        ({"distances": "notdict"}, TypeError, "must be a dict"),
        ({"distances": "bad"}, TypeError, "entry for node '0,0' must be a number"),
        ({"distances": "bad", "coords": "pos"}, ValueError, "not both"),
        ({"heuristic": "cosine"}, ValueError, "unknown heuristic 'cosine'"),
        ({"coords": 5}, TypeError, "coords must be"),
        ({"coords": []}, ValueError, "at least one dimension"),
        ({"heuristic": 3}, TypeError, "invalid value for option 'heuristic'"),
    ]
    for kwargs, error, text in cases:
        with pytest.raises(error, match=text):
            g.shortest_path("0,0", "2,2", method="astar", **kwargs)
    with pytest.raises(ValueError, match="target node 'nowhere' has no coordinates"):
        g.shortest_path("0,0", "nowhere", method="astar")

    g["1,0"].attr_set("x", "one")
    with pytest.raises(TypeError, match="coordinate 'x' of node '1,0' must be a number"):
        g.shortest_path("0,0", "2,2", method="astar")
    g["1,0"].attr_set("pos", "ab")
    with pytest.raises(TypeError, match="sequence of numbers"):
        g.shortest_path("0,0", "2,2", method="astar", coords="pos")
    g["1,0"].attr_set("pos", (1, 0, 0))
    with pytest.raises(ValueError, match="has 3 coordinates but the target has 2"):
        g.shortest_path("0,0", "2,2", method="astar", coords="pos")


def test_negative_and_non_numeric_weights():
    g = grid(2)
    g.add_edge("0,0", "1,1", {"weight": -1})
    with pytest.raises(ValueError, match="non-negative"):
        g.shortest_path("0,0", "1,1", method="astar")
    g = grid(2)
    g.add_edge("0,0", "1,1", {"weight": "heavy"})
    with pytest.raises(TypeError, match="must be a number"):
        g.shortest_path("0,0", "1,1", method="astar")


def test_traversal_results_stay_within_their_nodes():
    """A bfs() result shares nodes with the full graph; a path in it must not
    detour through nodes it doesn't contain."""
    g = Vertex()
    for n, x in (("a", 0), ("b", 2), ("c", 1)):
        g.add_node(n, {"x": x, "y": 0})
    g.add_edge("a", "b", {"type": "direct", "weight": 10})
    g.add_edge("a", "c", {"type": "detour", "weight": 1})
    g.add_edge("c", "b", {"type": "detour", "weight": 1})
    assert g.shortest_path("a", "b", method="astar").meta["cost"] == 2.0

    part = g["a"].bfs(filter={"type": "direct"})
    assert sorted(part.keys()) == ["a", "b"]
    for method in ("dijkstra", "astar"):
        assert part.shortest_path("a", "b", method=method).meta["nodelist"] == ["a", "b"]
