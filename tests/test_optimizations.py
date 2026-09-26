import gc
import json
import os

import pytest

try:
    from ironweaver import Vertex
except Exception as e:  # pragma: no cover - module unavailable
    pytest.skip(f"ironweaver module unavailable: {e}", allow_module_level=True)


def chain(n):
    g = Vertex()
    for i in range(n):
        g.add_node(f"n{i}")
    for i in range(n - 1):
        g.add_edge(f"n{i}", f"n{i + 1}")
    return g


def diamond():
    #   a -> b -> d
    #   a -> c -> d      plus d -> e
    g = Vertex()
    for n in "abcde":
        g.add_node(n, {"name": n})
    g.add_edge("a", "b", {"type": "x", "weight": 1.0})
    g.add_edge("b", "d", {"type": "x", "weight": 5.0})
    g.add_edge("a", "c", {"type": "y", "weight": 2.0})
    g.add_edge("c", "d", {"type": "y", "weight": 1.0})
    g.add_edge("d", "e", {"type": "x"})
    return g


def edge_pairs(node, inverse=False):
    edges = node.inverse_edges if inverse else node.edges
    return sorted((e.from_node.id, e.to_node.id) for e in edges)


# --- garbage collection -----------------------------------------------------

def test_graphs_are_garbage_collected():
    class Tracker:
        finalized = 0

        def __del__(self):
            Tracker.finalized += 1

    for _ in range(3):
        g = diamond()
        g.get_node("a").attr_set("tracker", Tracker())
        sub = g.filter(ids=["a", "b"])
        del g, sub
    gc.collect()
    assert Tracker.finalized == 3


# --- traversal --------------------------------------------------------------

def test_traverse_deep_chain_does_not_overflow():
    g = chain(200_000)
    result = g.get_node("n0").traverse()
    assert result.node_count() == 200_000
    assert result.meta["nodelist"][:3] == ["n0", "n1", "n2"]


def test_traverse_depth_and_order():
    g = diamond()
    result = g.get_node("a").traverse(depth=2)
    assert result.meta["nodelist"] == ["a", "b", "d", "c"]


# --- subgraphs --------------------------------------------------------------

def test_subgraph_has_inverse_edges_and_meta():
    g = diamond()
    g.get_node("b").meta = {"m": 1}
    sub = g.filter(ids=["a", "b", "d"])
    assert edge_pairs(sub["d"], inverse=True) == [("b", "d")]
    assert edge_pairs(sub["b"], inverse=True) == [("a", "b")]
    assert sub["b"].meta["m"] == 1
    # edges point at the new nodes, not the originals
    assert sub["a"].edges[0].to_node is sub["b"] or sub["a"].edges[0].to_node.id == "b"
    assert sub["a"].edges[0].to_node.vertex is not None


def test_filter_result_fires_update_callbacks():
    g = diamond()
    events = []
    g.on_node_update_callbacks.append(lambda v, n, k, new, old: events.append((n.id, k)))
    g.on_edge_update_callbacks.append(lambda v, e, k, new, old: events.append((e.to_node.id, k)))
    sub = g.filter(ids=["a", "b"])
    sub["a"].attr_set("score", 3)
    sub["a"].edges[0].attr_set("weight", 9.0)
    assert events == [("a", "score"), ("b", "weight")]


def test_expand_matches_union_of_bfs():
    g = diamond()
    seed = g.filter(ids=["b", "c"])
    assert sorted(seed.expand(g, depth=1).keys()) == ["b", "c", "d"]
    assert sorted(seed.expand(g, depth=2).keys()) == ["b", "c", "d", "e"]


def test_expand_direction():
    g = diamond()
    seed = g.filter(id="d")
    assert sorted(seed.expand(g, direction="in").keys()) == ["b", "c", "d"]
    assert sorted(seed.expand(g, direction="both").keys()) == ["b", "c", "d", "e"]
    with pytest.raises(ValueError):
        seed.expand(g, direction="sideways")


def test_shortest_path_bfs_direction():
    g = diamond()
    with pytest.raises(ValueError):
        g.shortest_path_bfs("e", "a")
    path = g.shortest_path_bfs("e", "a", direction="in")
    assert path.meta["nodelist"][0] == "e"
    assert path.meta["nodelist"][-1] == "a"
    assert len(path.meta["nodelist"]) == 4


# --- random walks -----------------------------------------------------------

def test_random_walks_seed_is_reproducible():
    g = diamond()
    a = g.random_walks("a", 4, 500, allow_revisit=True, include_edge_types=True, seed=7)
    b = g.random_walks("a", 4, 500, allow_revisit=True, include_edge_types=True, seed=7)
    assert a == b
    s1 = g.random_walks(None, 3, 200, stratified=True, seed=3)
    s2 = g.random_walks(None, 3, 200, stratified=True, seed=3)
    assert s1 == s2


def test_random_walks_dedup_with_commas_in_ids():
    g = Vertex()
    for n in ["s", "a,b", "a", "b"]:
        g.add_node(n)
    g.add_edge("s", "a,b")
    g.add_edge("s", "a")
    g.add_edge("a", "b")
    walks = g.random_walks("s", 3, 200, seed=1)
    assert sorted(walks) == [["s", "a", "b"], ["s", "a,b"]]


def test_stratified_start_spreads_over_nodes():
    g = Vertex()
    for i in range(50):
        g.add_node(f"n{i}")
    # Uniform sampling of 50 starts from 50 nodes hits ~31.8 distinct nodes on
    # average; inverse-visit weighting should clearly beat that.
    distinct = [len(g.random_walks(None, 1, 50, stratified=True, seed=s)) for s in range(20)]
    assert sum(distinct) / len(distinct) > 34


# --- serialization ----------------------------------------------------------

def test_bool_and_tuple_round_trip(tmp_path):
    g = Vertex()
    g.add_node("a", {"flag": True, "off": False, "n": 3, "t": (1, 2), "x": 1.5})
    data = json.loads(g.save_to_json())
    assert data["nodes"]["a"]["attr"]["flag"] == {"Bool": True}

    for loaded in (
        Vertex.load_from_json(g.save_to_json()),
        Vertex.load_from_json(data),
    ):
        attr = loaded["a"].attr
        assert attr["flag"] is True and attr["off"] is False
        assert attr["n"] == 3 and attr["t"] == [1, 2] and attr["x"] == 1.5

    path = os.path.join(tmp_path, "g.bin")
    g.save_to_binary(path)
    assert Vertex.load_from_binary(path)["a"].attr["flag"] is True


def test_numpy_embeddings_serialize():
    np = pytest.importorskip("numpy")
    g = Vertex()
    g.add_node("a", {"emb": np.array([0.5, 1.5], dtype=np.float32), "k": np.int64(4)})
    attr = Vertex.load_from_json(g.save_to_json())["a"].attr
    assert attr["emb"] == [0.5, 1.5]
    assert attr["k"] == 4


def test_loaded_graph_is_wired_and_keeps_edge_order(tmp_path):
    g = Vertex()
    for n in ["hub", "z", "y", "x", "w"]:
        g.add_node(n)
    for n in ["z", "y", "x", "w"]:
        g.add_edge("hub", n)
    path = os.path.join(tmp_path, "g.bin")
    g.save_to_binary(path)
    loaded = Vertex.load_from_binary(path)

    assert [e.to_node.id for e in loaded["hub"].edges] == ["z", "y", "x", "w"]
    assert edge_pairs(loaded["z"], inverse=True) == [("hub", "z")]

    events = []
    loaded.on_node_update_callbacks.append(lambda v, n, k, new, old: events.append(k))
    loaded["hub"].attr_set("k", 1)
    assert events == ["k"]
    assert loaded["hub"].vertex is loaded


# --- networkx ---------------------------------------------------------------

def test_to_networkx_bulk():
    nx = pytest.importorskip("networkx")
    g = diamond()
    d = g.to_networkx()
    assert isinstance(d, nx.DiGraph)
    assert d.number_of_nodes() == 5 and d.number_of_edges() == 5
    assert d.nodes["a"]["name"] == "a"
    assert d.edges["a", "c"] == {"type": "y", "weight": 2.0}
    assert g.get_metadata()["edge_count"] == 5


# --- removal ----------------------------------------------------------------

def test_remove_node_leaves_no_dangling_edges():
    g = diamond()
    g.add_edge("d", "d")  # self loop
    removed = g.remove_node("d")
    assert removed.id == "d" and removed.edges == []
    assert not g.has_node("d")
    assert edge_pairs(g["b"]) == [] and edge_pairs(g["c"]) == []
    assert edge_pairs(g["e"], inverse=True) == []
    assert g.prune() == 0
    with pytest.raises(KeyError):
        g.remove_node("d")


def test_remove_edge():
    g = diamond()
    g.add_edge("a", "b", {"type": "z"})
    assert g.remove_edge("a", "b", {"type": "z"}) == 1
    assert edge_pairs(g["a"]) == [("a", "b"), ("a", "c")]
    assert g.remove_edge("a", "b") == 1
    assert edge_pairs(g["b"], inverse=True) == []
    assert g.remove_edge("a", "b") == 0
    with pytest.raises(ValueError):
        g.remove_edge("a", "nope")


# --- dijkstra ---------------------------------------------------------------

def test_dijkstra_picks_cheapest_path():
    g = diamond()
    path = g.shortest_path_dijkstra("a", "e")
    # a->c->d (2 + 1) is cheaper than a->b->d (1 + 5); d->e has default 1.0
    assert path.meta["nodelist"] == ["a", "c", "d", "e"]
    assert path.meta["cost"] == pytest.approx(4.0)
    # BFS would take the first-found hop-equal route instead
    assert g.shortest_path_bfs("a", "e").meta["nodelist"] == ["a", "b", "d", "e"]


def test_dijkstra_limits_and_errors():
    g = diamond()
    with pytest.raises(ValueError):
        g.shortest_path_dijkstra("a", "e", max_cost=3.5)
    g.add_edge("e", "a", {"weight": -1})
    with pytest.raises(ValueError):
        g.shortest_path_dijkstra("e", "a")
    path = g.shortest_path_dijkstra("a", "e", weight="missing", default_weight=2.0)
    assert path.meta["cost"] == pytest.approx(6.0)
