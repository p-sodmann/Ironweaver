"""Projection.from_arrays: a projection built from node ids and edge arrays,
without a Vertex, gives the same results as one projected from a Vertex with
the same nodes and edges in the same order."""

import random

import pytest

from ironweaver import Projection, Vertex

np = pytest.importorskip("numpy")


def random_arrays(seed, n=120, m=600):
    """Ids in a shuffled order, edges with parallel edges and self-loops, a
    few isolated nodes, weights with ties."""
    rng = random.Random(seed)
    ids = [f"n{i}" for i in range(n)]
    rng.shuffle(ids)
    src = [rng.randrange(n - 5) for _ in range(m)]
    dst = [rng.randrange(n - 5) for _ in range(m)]
    for k in range(0, m, 37):
        dst[k] = src[k]  # self-loops
    for k in range(1, m, 41):
        src[k], dst[k] = src[k - 1], dst[k - 1]  # parallel edges
    weight = [float(rng.randint(1, 8)) for _ in range(m)]
    return ids, src, dst, weight


def vertex_from(ids, src, dst, weight):
    g = Vertex()
    g.add_nodes(ids)
    g.add_edges([(ids[s], ids[t]) for s, t in zip(src, dst)], attrs={"weight": weight})
    return g


def results(p):
    """Every query / analytics method of a projection, with fixed inputs."""
    ids = p.ids()
    rng = random.Random(7)
    pairs = [(rng.choice(ids), rng.choice(ids)) for _ in range(40)]
    out = {
        "repr": repr(p),
        "ids": ids,
        "counts": (p.node_count(), p.edge_count(), len(p), p.weighted, p.direction),
        "neighbors": [(p.neighbors(i), p.neighbors(i, "in"), p.degree(i), p.degree(i, "in")) for i in ids],
        "shortest_paths": p.shortest_paths(pairs),
        "shortest_paths_bfs": p.shortest_paths(pairs, "bfs"),
        "distances": p.distances(ids[:10], max_cost=12),
        "wcc": p.weakly_connected_components(),
        "scc": p.strongly_connected_components(),
        "find_cycle": p.find_cycle(),
        "degree_centrality": (p.degree_centrality(), p.degree_centrality("in")),
        "pagerank": p.pagerank(personalization={ids[0]: 1.0, ids[3]: 2.0}),
        "triangles": p.triangles(),
        "clustering": (p.clustering(), p.clustering(directed=True)),
        "core_number": p.core_number(),
        "label_propagation": p.label_propagation(),
        "bfs_levels": p.bfs_levels(ids[:3], max_depth=4),
        "betweenness": p.betweenness_centrality(),
        "betweenness_k": p.betweenness_centrality(20, seed=3, weighted=False),
        "closeness": p.closeness_centrality(),
        "harmonic": p.harmonic_centrality(weighted=False),
        "similarity": p.similarity(pairs, "adamic_adar"),
        "most_similar": p.most_similar(k=3),
        "leiden": p.leiden(seed=5),
        "mst": p.minimum_spanning_tree(),
        "fastrp": p.fastrp(16, seed=1),
        "node2vec": p.node2vec_walks(10, 2, p=0.5, q=2.0, seed=4),
        "ksp": p.k_shortest_paths(pairs[0][0], pairs[1][1], 4),
    }
    out["modularity"] = p.modularity(out["leiden"])
    try:
        out["topological_sort"] = p.topological_sort()
    except ValueError as e:
        out["topological_sort"] = str(e)
    return out


@pytest.mark.parametrize("direction", ["out", "in", "both"])
@pytest.mark.parametrize("weighted", [True, False])
def test_same_results_as_a_vertex_projection(direction, weighted):
    ids, src, dst, weight = random_arrays(1)
    g = vertex_from(ids, src, dst, weight)
    expected = g.project(weight="weight" if weighted else None, direction=direction)
    arrays = Projection.from_arrays(
        ids,
        np.array(src, dtype=np.int64),
        np.array(dst, dtype=np.int64),
        np.array(weight) if weighted else None,
        direction=direction,
    )
    assert results(arrays) == results(expected)


def test_dag_and_input_types():
    # A DAG with ties, so topological_sort has an answer; edges reversed to
    # make the order matter
    ids = [f"v{i}" for i in range(30)]
    edges = [(i, j) for i in range(30) for j in range(i + 1, min(i + 4, 30))][::-1]
    src, dst = [s for s, _ in edges], [t for _, t in edges]
    weight = [1.0 + (s * t) % 3 for s, t in edges]
    expected = results(vertex_from(ids, src, dst, weight).project(weight="weight"))
    assert isinstance(expected["topological_sort"], list)
    # int64 / int32 / strided arrays, float32 weights and plain lists
    for s, t, w in [
        (np.array(src), np.array(dst), np.array(weight)),
        (np.array(src, dtype=np.int32), np.array(dst, dtype=np.int32), np.array(weight, dtype=np.float32)),
        (np.array([src, dst]).T[:, 0], np.array([src, dst]).T[:, 1], np.repeat(weight, 2)[::2]),
        (src, dst, weight),
        (np.array(src, dtype=np.uint16), tuple(dst), [np.float64(x) for x in weight]),
    ]:
        assert results(Projection.from_arrays(ids, s, t, w)) == expected
    assert results(Projection.from_arrays(np.array(ids), src, dst, weight)) == expected


def test_edge_order_is_kept():
    # Ties between equally cheap paths and parallel edges with different
    # weights: in either edge order, the results are the Vertex's
    ids = ["a", "b", "c", "d"]
    src, dst, w = [0, 0, 1, 0, 0, 2], [1, 1, 3, 3, 2, 3], [2.0, 1.0, 1.0, 2.0, 1.0, 1.0]
    for order in [slice(None), slice(None, None, -1)]:
        s, t, x = src[order], dst[order], w[order]
        p = Projection.from_arrays(ids, s, t, x)
        q = vertex_from(ids, s, t, x).project(weight="weight")
        assert p.neighbors("a") == q.neighbors("a") == ["b", "b", "c", "d"]
        for method in ["dijkstra", "bfs"]:
            assert p.shortest_paths([("a", "d")], method) == q.shortest_paths([("a", "d")], method)
        assert p.k_shortest_paths("a", "d", 4) == q.k_shortest_paths("a", "d", 4)
        assert p.minimum_spanning_tree() == q.minimum_spanning_tree()


def test_unweighted_and_empty():
    p = Projection.from_arrays(["x", "y"], np.array([0]), np.array([1]))
    assert not p.weighted and p.edge_count() == 1 and p.shortest_paths([("x", "y")])[0]["cost"] == 1
    empty = Projection.from_arrays([], np.array([], dtype=np.int64), np.array([], dtype=np.int64), np.array([]))
    assert len(empty) == 0 and empty.weighted and empty.weakly_connected_components() == []
    assert "z" not in empty


def test_coords_are_accepted_and_checked():
    ids, src, dst = ["a", "b", "c"], np.array([0, 1]), np.array([1, 2])
    xy = np.array([[0.0, 0.0], [1.0, 0.0], [2.0, 1.0]])
    for coords in [xy, xy.astype(np.float32), xy.tolist(), np.asfortranarray(xy), None]:
        p = Projection.from_arrays(ids, src, dst, np.ones(2), coords)
        assert p.node_count() == 3
    with pytest.raises(ValueError, match="2 dimensions"):
        Projection.from_arrays(ids, src, dst, coords=np.zeros(3))
    with pytest.raises(ValueError, match="one row of 2 numbers per node"):
        Projection.from_arrays(ids, src, dst, coords=np.zeros((2, 2)))
    with pytest.raises(ValueError, match="at least one column"):
        Projection.from_arrays(ids, src, dst, coords=np.zeros((3, 0)))
    with pytest.raises(ValueError, match=r"finite, got NaN \(row 1, column 0\)"):
        Projection.from_arrays(ids, src, dst, coords=np.array([[0.0], [np.nan], [1.0]]))
    with pytest.raises(ValueError, match="equally long"):
        Projection.from_arrays(ids, src, dst, coords=[[0.0, 1.0], [1.0], [2.0, 2.0]])


def test_errors():
    ids = ["a", "b", "c"]
    src, dst = np.array([0, 1]), np.array([1, 2])
    with pytest.raises(ValueError, match="same length, got 2 and 1"):
        Projection.from_arrays(ids, src, dst[:1])
    with pytest.raises(ValueError, match=r"one entry per edge \(2\), got 3"):
        Projection.from_arrays(ids, src, dst, np.ones(3))
    with pytest.raises(ValueError, match=r"dst\[1\] is 3, not a node index \(0 to 2\)"):
        Projection.from_arrays(ids, src, np.array([1, 3]))
    with pytest.raises(ValueError, match=r"src\[0\] is -1"):
        Projection.from_arrays(ids, np.array([-1, 0]), dst)
    with pytest.raises(ValueError, match="no nodes"):
        Projection.from_arrays([], src, dst)
    with pytest.raises(ValueError, match=r"non-negative, got -1 \(weight\[1\]\)"):
        Projection.from_arrays(ids, src, dst, np.array([1.0, -1.0]))
    with pytest.raises(ValueError, match=r"non-negative, got NaN \(weight\[0\]\)"):
        Projection.from_arrays(ids, src, dst, np.array([np.nan, 1.0]))
    with pytest.raises(ValueError, match=r"duplicate node id 'a' \(ids\[0\] and ids\[2\]\)"):
        Projection.from_arrays(["a", "b", "a"], src, dst)
    with pytest.raises(ValueError, match="1 dimension, got 2"):
        Projection.from_arrays(ids, np.zeros((2, 1), dtype=np.int64), dst)
    with pytest.raises(TypeError, match="src must be an integer array"):
        Projection.from_arrays(ids, np.array([0.0, 1.0]), dst)
    with pytest.raises(TypeError, match="weight must be"):
        Projection.from_arrays(ids, src, dst, ["x", "y"])
    with pytest.raises(TypeError):
        Projection.from_arrays("abc", src, dst)
    with pytest.raises(ValueError, match="direction"):
        Projection.from_arrays(ids, src, dst, direction="sideways")
    # Infinite weights are allowed, as on Vertex edges
    p = Projection.from_arrays(ids, src, dst, np.array([1.0, np.inf]))
    assert p.shortest_paths([("a", "c")]) == vertex_from(ids, [0, 1], [1, 2], [1.0, np.inf]).project(
        weight="weight"
    ).shortest_paths([("a", "c")])
