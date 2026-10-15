"""Virtual endpoint routing uses one search, including entry and exit costs."""
import math
import random

import pytest

from ironweaver import Projection


def diamond():
    return Projection.from_arrays(
        ["a", "b", "c", "d", "isolated"],
        [0, 0, 1, 2], [1, 2, 3, 3], [1., 1., 1., 1.],
        coords=[[0.], [1.], [1.], [2.], [9.]],
    )


def test_terminal_penalties_and_real_goal():
    p = diamond()
    expected = {"nodelist": ["a", "b", "d"], "cost": 5., "source": "a", "target": "d"}
    for options in [{}, {"method": "astar"}, {"method": "astar", "goal_coords": [3.]}]:
        assert p.shortest_path_multi([("a", 3.), ("a", 2.), ("isolated", 0.)],
                                     [("b", 100.), ("d", 1.)], **options) == expected
        assert p.shortest_path_multi([("a", 2.)], [("d", 1.)], max_cost=4.9, **options) is None
    assert p.shortest_path_multi([("a", 2.)], [("d", 1.)], max_cost=5.) == expected
    stats = p.shortest_path_multi([("a", 0.)], [("d", 0.)], stats=True)
    assert stats["stats"]["settled"] > 0


def test_empty_unreachable_overlap_and_ties():
    p = diamond()
    assert p.shortest_path_multi([], [("d", 0.)]) is None
    assert p.shortest_path_multi([("a", 0.)], []) is None
    assert p.shortest_path_multi([("isolated", 0.)], [("d", 0.)]) is None
    for sources in [[("c", 0.), ("b", 0.)], [("b", 0.), ("c", 0.)]]:
        assert p.shortest_path_multi(sources, [("c", 0.), ("b", 0.)])["target"] == "b"
    r = p.shortest_path_multi([("a", 0.), ("d", 10.)], [("a", 100.), ("d", 0.)])
    assert (r["source"], r["target"], r["cost"]) == ("a", "d", 2.)


@pytest.mark.parametrize("bad", [-1., math.inf, math.nan])
def test_invalid_endpoint_costs(bad):
    p = diamond()
    for s, t in [([("a", bad)], [("d", 0.)]), ([("a", 0.)], [("d", bad)])]:
        with pytest.raises(ValueError, match="finite and non-negative"):
            p.shortest_path_multi(s, t)


def test_options_and_unweighted():
    p = diamond()
    for options in [{"method": "bfs"}, {"method": "dijkstra", "goal_coords": [3.]},
                    {"method": "astar", "goal_coords": [math.nan]},
                    {"method": "astar", "goal_coords": [2., 3.]},
                    {"heuristic_scale": -1.}, {"max_cost": -1.}]:
        with pytest.raises(ValueError):
            p.shortest_path_multi([("a", 0.)], [("d", 0.)], **options)
    with pytest.raises(ValueError, match="not found"):
        p.shortest_path_multi([("absent", 0.)], [])
    p = Projection.from_arrays(["a", "b"], [0], [1])
    assert p.shortest_path_multi([("a", .5)], [("b", .25)])["cost"] == 1.75
    with pytest.raises(ValueError, match="coordinates"):
        p.shortest_path_multi([("a", 0.)], [("b", 0.)], "astar")


def test_random_graphs_match_pairwise_reference():
    rng = random.Random(124)
    for _ in range(12):
        n = 20
        src, dst, weight = [], [], []
        for _ in range(70):
            src.append(rng.randrange(n))
            dst.append(rng.randrange(n))
            weight.append(float(rng.randrange(6)))
        p = Projection.from_arrays(list(map(str, range(n))), src, dst, weight)
        sources = [(str(rng.randrange(n)), float(rng.randrange(6))) for _ in range(4)]
        targets = [(str(rng.randrange(n)), float(rng.randrange(6))) for _ in range(4)]
        pairs = [(s, t) for s, _ in sources for t, _ in targets]
        costs = [entry + path["cost"] + exit for (_, entry) in sources for (_, exit) in targets
                 for path in [p.shortest_paths([pairs.pop(0)])[0]] if path is not None]
        r = p.shortest_path_multi(sources, targets)
        assert (None if r is None else r["cost"]) == (min(costs) if costs else None)
        if r:
            assert r["nodelist"][0] == r["source"]
            assert r["nodelist"][-1] == r["target"]
            assert len(set(r["nodelist"])) == len(r["nodelist"])
