"""A* on projections: Projection.shortest_paths(..., "astar") with coordinates
compiled in by Vertex.project(coords=...) or Projection.from_arrays(coords=...),
and Vertex.shortest_paths(..., "astar"). With an admissible estimate the
costs equal Dijkstra's."""

import math
import random

import pytest

from ironweaver import Projection, Vertex


def grid_vertex(w, h, seed, walls=0.15):
    """A w x h 4-connected grid with walls; moving onto a tile costs its cost
    (1 to 4), so the Manhattan distance never overestimates."""
    rng = random.Random(seed)
    g = Vertex()
    cost = {}
    for y in range(h):
        for x in range(w):
            if rng.random() < walls and (x, y) not in [(0, 0), (w - 1, h - 1)]:
                continue
            g.add_node(f"{x},{y}", {"x": x, "y": y, "pos": [float(x), float(y)]})
            cost[(x, y)] = float(rng.randint(1, 4))
    for (x, y) in cost:
        for dx, dy in [(1, 0), (-1, 0), (0, 1), (0, -1)]:
            if (x + dx, y + dy) in cost:
                g.add_edge(f"{x},{y}", f"{x + dx},{y + dy}", {"cost": cost[(x + dx, y + dy)]})
    return g


def random_pairs(ids, k, seed):
    rng = random.Random(seed)
    return [(rng.choice(ids), rng.choice(ids)) for _ in range(k)]


def costs(results):
    return [r and r["cost"] for r in results]


@pytest.mark.parametrize("seed", range(4))
@pytest.mark.parametrize("direction", ["out", "in", "both"])
def test_astar_matches_dijkstra_on_grids(seed, direction):
    g = grid_vertex(25, 20, seed)
    p = g.project(weight="cost", coords=["x", "y"], direction=direction)
    assert p.coord_dim == 2
    pairs = random_pairs(p.ids(), 80, seed) + [("0,0", "24,19")]
    want = costs(p.shortest_paths(pairs, "dijkstra"))
    for heuristic in ["manhattan", "euclidean"]:
        got = p.shortest_paths(pairs, "astar", heuristic=heuristic)
        assert costs(got) == want
        for r, (s, t) in zip(got, pairs):
            if r is not None:
                assert (r["nodelist"][0], r["nodelist"][-1]) == (s, t)
    # A smaller scale is still admissible; 0 is Dijkstra
    for scale in [0.0, 0.5]:
        assert costs(p.shortest_paths(pairs, "astar", heuristic="manhattan", heuristic_scale=scale)) == want


def test_astar_paths_are_valid_and_cheapest():
    g = grid_vertex(15, 15, 3)
    p = g.project(weight="cost", coords="pos")
    pairs = random_pairs(p.ids(), 40, 1)
    for r, (s, t) in zip(p.shortest_paths(pairs, heuristic="manhattan"), pairs):
        if r is None:
            assert g.shortest_path(s, t, method="dijkstra", weight="cost") is None
            continue
        total = 0.0
        for a, b in zip(r["nodelist"], r["nodelist"][1:]):
            total += min(e.attr["cost"] for e in g.get_node(a).edges if e.to_node.id == b)
        assert total == r["cost"]
        assert r["cost"] == g.shortest_path(s, t, method="dijkstra", weight="cost").meta["cost"]


def test_vertex_shortest_paths_astar():
    g = grid_vertex(12, 10, 5)
    pairs = random_pairs(list(g.nodes), 30, 2)
    want = costs(g.shortest_paths(pairs, "dijkstra", weight="cost"))
    # coords default to ["x", "y"]; giving heuristic picks astar
    assert costs(g.shortest_paths(pairs, weight="cost", heuristic="manhattan")) == want
    assert costs(g.shortest_paths(pairs, "astar", weight="cost", coords="pos")) == want
    assert costs(g.shortest_paths(pairs, weight="cost", coords=["x", "y"], direction="in")) == costs(
        g.shortest_paths(pairs, "dijkstra", weight="cost", direction="in")
    )


def test_from_arrays_coords_and_max_cost():
    # A line 0 - 1 - 2 - 3 with unit steps, plus a costly shortcut 0 -> 3
    ids = ["a", "b", "c", "d"]
    src, dst = [0, 1, 2, 0], [1, 2, 3, 3]
    p = Projection.from_arrays(ids, src, dst, [1.0, 1.0, 1.0, 5.0], [[0.0], [1.0], [2.0], [3.0]])
    assert p.coord_dim == 1
    r = p.shortest_paths([("a", "d"), ("d", "a"), ("b", "b")], "astar", heuristic="manhattan")
    assert r == [{"nodelist": ["a", "b", "c", "d"], "cost": 3.0}, None, {"nodelist": ["b"], "cost": 0.0}]
    assert p.shortest_paths([("a", "d")], "astar", max_cost=2.5) == [None]
    assert p.shortest_paths([("a", "d")], "astar", max_cost=3.0)[0]["cost"] == 3.0
    # Reversed: the same estimates work on an "in" projection
    back = Projection.from_arrays(ids, src, dst, [1.0, 1.0, 1.0, 5.0], [[0.0], [1.0], [2.0], [3.0]], direction="in")
    assert back.shortest_paths([("d", "a")], "astar")[0]["nodelist"] == ["d", "c", "b", "a"]


def test_inadmissible_scale_may_cost_more():
    # Scaling the estimate up trades optimality for speed
    g = grid_vertex(20, 20, 7, walls=0.0)
    p = g.project(weight="cost", coords=["x", "y"])
    pairs = random_pairs(p.ids(), 30, 3)
    exact = costs(p.shortest_paths(pairs, "dijkstra"))
    rough = costs(p.shortest_paths(pairs, "astar", heuristic="manhattan", heuristic_scale=4.0))
    assert all(r >= e for r, e in zip(rough, exact))


def test_project_coords_errors():
    g = Vertex()
    g.add_node("a", {"x": 0, "y": 1})
    g.add_node("b", {"x": 1})
    g.add_edge("a", "b", {"w": 1.0})
    with pytest.raises(ValueError, match=r"node 'b' has no coordinates at coords=\['x', 'y'\]"):
        g.project(weight="w", coords=["x", "y"])
    # Filtered out nodes need none
    assert g.project(weight="w", coords=["x", "y"], nodes=["a"]).coord_dim == 2
    g.get_node("b").attr_set("y", "north")
    with pytest.raises(TypeError, match="coordinate 'y' of node 'b' must be a number"):
        g.project(coords=["x", "y"])
    g.get_node("a").attr_set("pos", [0.0, 1.0])
    g.get_node("b").attr_set("pos", [0.0, 1.0, 2.0])
    with pytest.raises(ValueError, match="node 'b' has 3 coordinates but node 'a' has 2"):
        g.project(coords="pos")
    g.get_node("b").attr_set("pos", [0.0, math.inf])
    with pytest.raises(ValueError, match="finite"):
        g.project(coords="pos")
    with pytest.raises(TypeError, match="coords must be"):
        g.project(coords=3)
    assert g.project().coord_dim == 0


def test_astar_option_errors():
    g = grid_vertex(4, 4, 0, walls=0.0)
    p = g.project(weight="cost", coords=["x", "y"])
    pair = [("0,0", "3,3")]
    with pytest.raises(ValueError, match="needs a projection with node coordinates"):
        g.project(weight="cost").shortest_paths(pair, "astar")
    with pytest.raises(ValueError, match="unknown heuristic 'chebyshev'; use 'euclidean' or 'manhattan'"):
        p.shortest_paths(pair, "astar", heuristic="chebyshev")
    for scale in [-1.0, math.nan, math.inf]:
        with pytest.raises(ValueError, match="heuristic_scale must be finite and non-negative"):
            p.shortest_paths(pair, "astar", heuristic_scale=scale)
    with pytest.raises(ValueError, match="options of method 'astar', not 'dijkstra'"):
        p.shortest_paths(pair, "dijkstra", heuristic="manhattan")
    with pytest.raises(ValueError, match="options of method 'astar', not 'dijkstra'"):
        g.shortest_paths(pair, "dijkstra", weight="cost", coords=["x", "y"])
    with pytest.raises(ValueError, match="needs a target per query"):
        p.distances(["0,0"], method="astar")
    with pytest.raises(TypeError):
        g.distances(["0,0"], method="dijkstra", heuristic="manhattan")
