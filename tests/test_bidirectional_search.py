"""Bidirectional search in shortest_path_bfs / Node.bfs_search, checked
against networkx and against the forward-only traversal on random graphs."""

import random

import pytest

nx = pytest.importorskip("networkx")

try:
    from ironweaver import Edge, Node, Vertex
except Exception as e:  # pragma: no cover - module unavailable
    pytest.skip(f"ironweaver module unavailable: {e}", allow_module_level=True)

TYPES = ("a", "b")


def random_graph(seed, n=40, m=70):
    rng = random.Random(seed)
    g = Vertex()
    ref = nx.MultiDiGraph()
    for i in range(n):
        g.add_node(f"n{i}")
        ref.add_node(f"n{i}")
    for _ in range(m):
        u, v, t = f"n{rng.randrange(n)}", f"n{rng.randrange(n)}", rng.choice(TYPES)
        g.add_edge(u, v, {"type": t})
        ref.add_edge(u, v, type=t)
    return g, ref


def reference_view(ref, direction):
    if direction == "out":
        return ref
    if direction == "in":
        return ref.reverse(copy=False)
    return ref.to_undirected(as_view=True)


def hop_exists(g, u, v, direction):
    out = any(e.to_node.id == v for e in g[u].edges)
    inc = any(e.from_node.id == v for e in g[u].inverse_edges)
    return {"out": out, "in": inc, "both": out or inc}[direction]


@pytest.mark.parametrize("seed", range(50))
@pytest.mark.parametrize("direction", ["out", "in", "both"])
def test_shortest_path_matches_networkx(seed, direction):
    g, ref = random_graph(seed)
    view = reference_view(ref, direction)
    rng = random.Random(1000 + seed)
    for _ in range(10):
        src, dst = f"n{rng.randrange(40)}", f"n{rng.randrange(40)}"
        max_depth = rng.choice([None, 1, 2, 3, 5])
        try:
            expected = nx.shortest_path_length(view, src, dst)
        except nx.NetworkXNoPath:
            expected = None
        if expected is not None and max_depth is not None and expected > max_depth:
            expected = None

        if expected is None:
            with pytest.raises(ValueError):
                g.shortest_path_bfs(src, dst, max_depth=max_depth, direction=direction)
            continue

        path = g.shortest_path_bfs(src, dst, max_depth=max_depth, direction=direction).meta["nodelist"]
        assert path[0] == src and path[-1] == dst
        assert len(path) - 1 == expected
        assert len(set(path)) == len(path)
        for u, v in zip(path, path[1:]):
            assert hop_exists(g, u, v, direction), (u, v, direction)


@pytest.mark.parametrize("seed", range(30))
def test_bfs_search_agrees_with_forward_traversal(seed):
    g, _ = random_graph(seed)
    rng = random.Random(2000 + seed)
    for _ in range(10):
        src, dst = f"n{rng.randrange(40)}", f"n{rng.randrange(40)}"
        depth = rng.choice([None, 1, 2, 4])
        start = g.get_node(src)
        for kwargs in ({}, {"filter": {"type": "a"}}, {"filter": lambda e: e.type == "b"}):
            reachable = dst in start.bfs(depth=depth, **kwargs).keys()
            found = start.bfs_search(dst, depth=depth, **kwargs)
            assert (found is not None) == reachable, (src, dst, depth, kwargs)
            if found is not None:
                assert found.id == dst


def test_standalone_nodes_and_edges():
    # A Node built directly lives in its own one-node Vertex; edges can only
    # be made through a Vertex, so edges/inverse_edges always stay in sync.
    a = Node("a", {"k": 1})
    assert a.vertex.keys() == ["a"] and a.attr == {"k": 1}
    assert a.bfs_search("a") == a
    assert a.bfs_search("b") is None
    with pytest.raises(TypeError):
        Edge(a, a, None, None)
    with pytest.raises(TypeError):
        Node("b", None, [object()])


def test_shortest_path_on_long_chain():
    g = Vertex()
    for i in range(2000):
        g.add_node(f"n{i}")
    for i in range(1999):
        g.add_edge(f"n{i}", f"n{i + 1}")
    path = g.shortest_path_bfs("n0", "n1999").meta["nodelist"]
    assert path == [f"n{i}" for i in range(2000)]
    with pytest.raises(ValueError):
        g.shortest_path_bfs("n0", "n1999", max_depth=1998)
    assert len(g.shortest_path_bfs("n0", "n1999", max_depth=1999).meta["nodelist"]) == 2000
