"""Vertex.project / Projection: a compact, read-only copy of the graph that
analytics queries run on in parallel."""

import random
import threading

import pytest

try:
    from ironweaver import EdgeView, NodeView, Projection, Vertex, attr, edge_type, label
except Exception as e:  # pragma: no cover - module unavailable
    pytest.skip(f"ironweaver module unavailable: {e}", allow_module_level=True)


def random_graph(seed, n=200, m=900):
    rng = random.Random(seed)
    g = Vertex()
    for i in range(n):
        g.add_node(f"n{i}", {"group": i % 3})
    for _ in range(m):
        a, b = rng.randrange(n), rng.randrange(n)
        g.add_edge(f"n{a}", f"n{b}", {"weight": rng.uniform(0.1, 10), "type": rng.choice("xy")})
    return g


def edge_list(g):
    return [(n.id, e.to_node.id) for n in g for e in n.edges]


def small():
    #   a -> b (twice), b -> c, c -> a, a -> a;  d isolated
    g = Vertex()
    for n in "abcd":
        g.add_node(n, {"kind": "z" if n == "d" else "k"})
    g.add_edge("a", "b", {"weight": 2.0, "type": "x"})
    g.add_edge("a", "b", {"weight": 1.0, "type": "y"})
    g.add_edge("b", "c", {"weight": 3.0, "type": "x"})
    g.add_edge("c", "a", {"type": "x"})
    g.add_edge("a", "a", {"weight": 4.0, "type": "y"})
    return g


def test_structure_and_metadata():
    g = small()
    p = g.project()
    assert isinstance(p, Projection)
    assert (len(p), p.node_count(), p.edge_count()) == (4, 4, 5)
    assert (p.direction, p.weighted) == ("out", False)
    assert p.ids() == ["a", "b", "c", "d"]
    assert "a" in p and "zz" not in p
    # Sorted neighbour lists; parallel edges repeat the neighbour
    assert p.neighbors("a") == ["a", "b", "b"]
    assert p.neighbors("a", "in") == ["a", "c"]
    assert (p.degree("a"), p.degree("a", "in"), p.degree("d")) == (3, 2, 0)
    assert p.memory_usage() > 0
    assert repr(p) == "Projection(nodes=4, edges=5, direction='out', weighted=False)"

    rev = g.project(direction="in")
    assert rev.neighbors("a") == ["a", "c"]
    both = g.project(direction="both")
    assert both.edge_count() == 5
    assert both.neighbors("a") == ["a", "a", "b", "b", "c"]  # the self-loop from both ends
    assert both.neighbors("a", "in") == both.neighbors("a")


@pytest.mark.parametrize("direction", ["out", "in", "both"])
def test_adjacency_matches_the_graph(direction):
    g = random_graph(7)
    p = g.project(direction=direction)
    expected = {n.id: [] for n in g}
    for a, b in edge_list(g):
        if direction in ("out", "both"):
            expected[a].append(b)
        if direction in ("in", "both"):
            expected[b].append(a)
    order = {id: i for i, id in enumerate(p.ids())}
    for id, nbrs in expected.items():
        got = p.neighbors(id)
        assert got == sorted(got, key=order.__getitem__)
        assert sorted(got) == sorted(nbrs)
        assert p.degree(id) == len(nbrs)


def test_weights_and_queries():
    g = small()
    p = g.project(weight="weight")  # c -> a has no weight: default 1.0
    assert p.weighted
    res = p.shortest_paths([("a", "c"), ("c", "b"), ("d", "a"), ("b", "b")])
    assert res[0] == {"nodelist": ["a", "b", "c"], "cost": 4.0}  # the cheaper parallel edge
    assert res[1] == {"nodelist": ["c", "a", "b"], "cost": 2.0}
    assert res[2] is None
    assert res[3] == {"nodelist": ["b"], "cost": 0.0}
    assert p.shortest_paths([("a", "c")], "bfs") == [{"nodelist": ["a", "b", "c"], "cost": 2}]
    assert p.distances(["a"], max_cost=3.5) == {"a": {"a": 0.0, "b": 1.0}}
    assert p.distances(["c"], targets=["b"]) == {"c": {"b": 2.0}}
    assert g.project(default_weight=2.0).distances(["c"])["c"]["a"] == 2.0


def test_projection_queries_match_vertex_queries():
    g = random_graph(3)
    rng = random.Random(5)
    ids = [n.id for n in g]
    pairs = [(rng.choice(ids), rng.choice(ids)) for _ in range(60)]
    for direction in ["out", "in", "both"]:
        p = g.project(weight="weight", direction=direction)
        assert p.shortest_paths(pairs) == g.shortest_paths(pairs, weight="weight", direction=direction)
        assert p.distances(ids[:5]) == g.distances(ids[:5], weight="weight", direction=direction)
        hops = g.project(direction=direction)
        assert hops.shortest_paths(pairs) == g.shortest_paths(pairs, direction=direction)


def test_filters():
    g = small()
    by_dict = g.project(node_filter={"kind": "k"}, edge_filter={"type": "x"})
    assert by_dict.ids() == ["a", "b", "c"]
    assert (by_dict.edge_count(), by_dict.neighbors("a")) == (3, ["b"])

    seen = []

    def edge_ok(e):
        seen.append(e)
        return e.type == "x"

    by_call = g.project(node_filter=lambda n: n.attr("kind") == "k", edge_filter=edge_ok, direction="both")
    assert by_call.ids() == ["a", "b", "c"]
    assert by_call.edge_count() == 3
    assert all(isinstance(e, EdgeView) for e in seen)
    assert len(seen) == 5  # asked once per edge between kept nodes, even for "both"

    views = []
    g.project(node_filter=lambda n: views.append(n) or True)
    assert all(isinstance(v, NodeView) for v in views) and len(views) == 4

    sub = g.project(nodes=["a", "b", "d"], weight="weight")
    assert sub.ids() == ["a", "b", "d"]
    assert sub.shortest_paths([("a", "b"), ("b", "a")]) == [{"nodelist": ["a", "b"], "cost": 1.0}, None]

    # A bad weight on a filtered-out edge doesn't matter
    g.add_edge("d", "a", {"weight": "heavy"})
    with pytest.raises(TypeError):
        g.project(weight="weight")
    assert g.project(weight="weight", nodes=["a", "b", "c"]).edge_count() == 5


@pytest.mark.parametrize("node_kind", ["none", "empty", "dict", "expr", "callable"])
@pytest.mark.parametrize("edge_kind", ["none", "empty", "dict", "expr", "callable"])
@pytest.mark.parametrize("direction", ["out", "in", "both"])
def test_filter_forms_compose(node_kind, edge_kind, direction):
    g = small()
    for nid in "abc":
        g[nid].add_label("Keep")
    nodes = {
        "none": None,
        "empty": {},
        "dict": {"kind": "k", "labels": ["Keep"]},
        "expr": (attr("kind") == "k") & label("Keep"),
        "callable": lambda n: n.attr("kind") == "k",
    }
    edges = {
        "none": None,
        "empty": {},
        "dict": {"type": "x"},
        "expr": edge_type("x"),
        "callable": lambda e: e.type == "x",
    }
    p = g.project(node_filter=nodes[node_kind], edge_filter=edges[edge_kind], direction=direction)
    expected_ids = list("abcd" if node_kind in ("none", "empty") else "abc")
    expected_edges = [("a", "b"), ("b", "c"), ("c", "a")]
    if edge_kind in ("none", "empty"):
        expected_edges += [("a", "b"), ("a", "a")]
    assert p.ids() == expected_ids
    assert p.edge_count() == len(expected_edges)
    for nid in expected_ids:
        neighbors = []
        for source, target in expected_edges:
            if direction in ("out", "both") and source == nid:
                neighbors.append(target)
            if direction in ("in", "both") and target == nid:
                neighbors.append(source)
        assert p.neighbors(nid) == sorted(neighbors)


def test_filter_callbacks_only_visit_selected_nodes_and_edges():
    g = small()
    seen = []

    def node_ok(n):
        assert isinstance(n, NodeView)
        seen.append(("node", n.id))
        return [True]  # Callable results use Python truthiness.

    def edge_ok(e):
        assert isinstance(e, EdgeView)
        seen.append(("edge", e.from_node.id, e.to_node.id, e.type))
        return [True]

    p = g.project(nodes=["a", "b"], node_filter=node_ok, edge_filter=edge_ok, direction="both")
    assert (p.ids(), p.edge_count()) == (["a", "b"], 3)
    assert seen == [
        ("node", "a"), ("node", "b"),
        ("edge", "a", "b", "x"), ("edge", "a", "b", "y"), ("edge", "a", "a", "y"),
    ]


@pytest.mark.parametrize("slot", ["node_filter", "edge_filter"])
@pytest.mark.parametrize("where", ["callback", "truthiness"])
def test_filter_callback_errors_propagate_unchanged(slot, where):
    failure = LookupError("filter failed")

    class BadTruth:
        def __bool__(self):
            raise failure

    def predicate(value):
        if where == "callback":
            raise failure
        return BadTruth()

    with pytest.raises(LookupError) as caught:
        small().project(**{slot: predicate})
    assert caught.value is failure


def test_is_a_snapshot():
    g = small()
    p = g.project(weight="weight")
    g.remove_node("b")
    g.add_node("e")
    assert p.ids() == ["a", "b", "c", "d"]
    assert p.shortest_paths([("a", "c")])[0]["nodelist"] == ["a", "b", "c"]


def test_errors():
    g = small()
    p = g.project()
    with pytest.raises(ValueError, match="Target node with id 'zz' not found in the projection"):
        p.shortest_paths([("a", "zz")])
    with pytest.raises(ValueError, match="Root node"):
        p.distances(["zz"])
    with pytest.raises(ValueError, match="Node with id 'zz' not found"):
        p.neighbors("zz")
    with pytest.raises(ValueError, match="direction"):
        p.neighbors("a", "both")
    with pytest.raises(ValueError, match="need a projection with edge weights"):
        p.shortest_paths([("a", "c")], "dijkstra")
    with pytest.raises(ValueError, match="astar"):
        p.shortest_paths([("a", "c")], "astar")
    with pytest.raises(ValueError, match="unknown path method"):
        p.distances(["a"], method="nope")
    with pytest.raises(ValueError, match="max_cost"):
        p.distances(["a"], max_cost=-1)
    with pytest.raises(ValueError, match="Node with id 'zz' not found"):
        g.project(nodes=["zz"])
    with pytest.raises(ValueError, match="direction"):
        g.project(direction="sideways")
    with pytest.raises(TypeError, match="node_filter"):
        g.project(node_filter=3)
    g.add_edge("a", "c", {"weight": -1.0})
    with pytest.raises(ValueError, match="non-negative"):
        g.project(weight="weight")
    # A filter that raises propagates
    with pytest.raises(ZeroDivisionError):
        g.project(edge_filter=lambda e: 1 / 0)


def test_queries_from_several_threads():
    g = random_graph(11, n=2000, m=10000)
    p = g.project(weight="weight")
    ids = p.ids()
    expected = p.distances(ids[:8])
    results = [None] * 4

    def work(i):
        results[i] = p.distances(ids[:8])

    threads = [threading.Thread(target=work, args=(i,)) for i in range(4)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert all(r == expected for r in results)
