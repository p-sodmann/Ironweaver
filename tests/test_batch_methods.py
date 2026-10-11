"""Method selection stays consistent across Vertex and Projection queries."""

import pytest

from ironweaver import Vertex


QUERIES = [
    "vertex_paths",
    "vertex_distances",
    "projection_paths",
    "projection_distances",
    "projection_k_paths",
]


@pytest.fixture
def graph():
    g = Vertex()
    g.add_nodes(["a", "b", "c"])
    # The cheapest path takes two hops; the fewest-hop path takes one.
    g.add_edge("a", "b", {"weight": 5.0})
    g.add_edge("b", "c", {"weight": 5.0})
    g.add_edge("a", "c", {"weight": 20.0})
    return g


def query(graph, kind, method, weighted=False, source="a", **options):
    if kind.startswith("vertex_"):
        obj = graph
        if weighted:
            options["weight"] = "weight"
    else:
        obj = graph.project(weight="weight" if weighted else None)
    if kind.endswith("distances"):
        return obj.distances([source], ["c"], method=method, **options)[source]["c"]
    if kind == "projection_k_paths":
        return obj.k_shortest_paths(source, "c", 1, method=method, **options)[0]["cost"]
    return obj.shortest_paths([(source, "c")], method=method, **options)[0]["cost"]


@pytest.mark.parametrize("kind", QUERIES)
@pytest.mark.parametrize(
    "method,weighted,cost",
    [(None, False, 1), ("bfs", False, 1), (None, True, 10.0), ("dijkstra", True, 10.0)],
)
def test_batch_method_selection(graph, kind, method, weighted, cost):
    actual = query(graph, kind, method, weighted)
    assert actual == cost
    assert type(actual) is type(cost)


@pytest.mark.parametrize("kind", QUERIES)
@pytest.mark.parametrize("method", ["astar", "nope", ""])
def test_batch_method_errors(graph, kind, method):
    with pytest.raises(ValueError) as exc:
        query(graph, kind, method)
    if method == "astar":
        expected = "method 'astar' is not available for batch queries; use 'bfs' or 'dijkstra'"
    else:
        expected = f"unknown path method '{method}'; available: 'bfs', 'dijkstra', 'astar'"
    assert str(exc.value) == expected


@pytest.mark.parametrize("kind", QUERIES[:-1])
@pytest.mark.parametrize("method", ["astar", "nope"])
def test_method_errors_precede_bad_limits_and_missing_nodes(graph, kind, method):
    with pytest.raises(ValueError) as exc:
        query(graph, kind, method, source="missing", max_cost=-1)
    prefix = f"method '{method}'" if method == "astar" else f"unknown path method '{method}'"
    assert str(exc.value).startswith(prefix)
