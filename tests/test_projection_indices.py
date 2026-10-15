"""Dense integer handles avoid string input/output conversion (#125)."""
import pytest

from ironweaver import Projection, Vertex


@pytest.fixture
def projection():
    return Projection.from_arrays(["2", "0", "1", "isolated"], [0, 1, 1, 2], [1, 0, 2, 2])


@pytest.mark.parametrize("method", [None, "bfs"])
def test_index_paths_match_string_paths(projection, method):
    pairs = [(0, 2), (2, 0), (3, 3), (0, 2)]
    ids = projection.ids()
    expected = projection.shortest_paths([(ids[s], ids[t]) for s, t in pairs], method, stats=True)
    for path in expected:
        if path is not None:
            path["nodelist"] = [ids.index(node) for node in path["nodelist"]]
    assert projection.shortest_paths(pairs, method, by_index=True, stats=True) == expected
    assert projection.shortest_paths([], by_index=True) == []
    assert projection.shortest_paths([(0, 2)], by_index=True, max_cost=1) == [None]


@pytest.mark.parametrize("method", ["dijkstra", "astar"])
def test_weighted_index_paths(method):
    p = Projection.from_arrays(["a", "b", "c"], [0, 1], [1, 2], [2., 3.], coords=[[0.], [1.], [2.]])
    assert p.shortest_paths([(0, 2)], method, by_index=True) == [{"nodelist": [0, 1, 2], "cost": 5.}]


@pytest.mark.parametrize("method", ["weakly_connected_components", "strongly_connected_components"])
def test_index_components(projection, method):
    ids = projection.ids()
    groups = getattr(projection, method)()
    assert getattr(projection, method)(by_index=True) == [[ids.index(node) for node in group] for group in groups]
    assert getattr(Projection.from_arrays([], [], []), method)(by_index=True) == []


@pytest.mark.parametrize("endpoint", [-1, 4, 2**32])
def test_out_of_range(projection, endpoint):
    with pytest.raises(ValueError, match="node index"):
        projection.shortest_paths([(0, endpoint)], by_index=True)


@pytest.mark.parametrize("endpoint", [True, False, 1.0, "1", None, 2**100])
def test_noninteger(projection, endpoint):
    with pytest.raises(TypeError, match="node indices"):
        projection.shortest_paths([(endpoint, 2)], by_index=True)


@pytest.mark.parametrize("pairs", [[(0,)], [(0, 1, 2)]])
def test_pair_shape(projection, pairs):
    with pytest.raises(ValueError, match="exactly two"):
        projection.shortest_paths(pairs, by_index=True)


def test_numpy_pairs(projection):
    np = pytest.importorskip("numpy")
    for dtype in [np.int32, np.int64]:
        pairs = np.array([[0, 2], [3, 3], [0, 1], [2, 0]], dtype=dtype)
        assert projection.shortest_paths(pairs[::2], by_index=True) == projection.shortest_paths([(0, 2), (0, 1)], by_index=True)
        assert projection.shortest_paths(np.empty((0, 2), dtype=dtype), by_index=True) == []
        for bad in [np.array([0, 2], dtype=dtype), np.zeros((1, 3), dtype=dtype)]:
            with pytest.raises(ValueError):
                projection.shortest_paths(bad, by_index=True)
    assert projection.shortest_paths([(np.int32(0), np.int64(2))], by_index=True) == [
        {"nodelist": [0, 1, 2], "cost": 2}
    ]


@pytest.mark.parametrize("kind", ["float_matrix", "bool_matrix", "false_scalar", "true_scalar", "bool_array_scalar"])
def test_numpy_noninteger_pairs(projection, kind):
    np = pytest.importorskip("numpy")
    pairs = {
        "float_matrix": np.array([[0., 2.]]),
        "bool_matrix": np.array([[False, True]]),
        "false_scalar": [(np.bool_(False), 2)],
        "true_scalar": [(np.bool_(True), 2)],
        "bool_array_scalar": [(np.array(False), 2)],
    }[kind]
    with pytest.raises(TypeError, match="node indices"):
        projection.shortest_paths(pairs, by_index=True)


def test_snapshot_indices_and_default_mode():
    g = Vertex()
    g.add_node("a")
    g.add_node("b")
    g.add_edge("a", "b")
    p = g.project()
    g.remove_node("a")
    g.add_node("new")
    assert p.shortest_paths([(0, 1)], by_index=True) == [{"nodelist": [0, 1], "cost": 1}]
    assert p.shortest_paths([("a", "b")]) == [{"nodelist": ["a", "b"], "cost": 1}]
    with pytest.raises(TypeError):
        p.shortest_paths([(0, 1)])
