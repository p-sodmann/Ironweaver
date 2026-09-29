"""Bulk loading: Vertex.add_nodes / Vertex.add_edges."""

import pytest

from ironweaver import Vertex


def test_add_nodes():
    g = Vertex()
    assert g.add_nodes(["a", ("b", {"age": 3}), ["c", {"labels": ["X"]}]], labels=["P"]) == 3
    assert g.keys() == ["a", "b", "c"]
    assert g["b"].attr == {"age": 3, "labels": ["P"]}
    assert g["c"].labels == ["P", "X"]
    # Attribute columns; None leaves the attribute out; "labels" sets labels
    assert g.add_nodes(["d", "e"], attrs={"age": [1, None], "labels": [["Q"], None]}) == 2
    assert g["d"].attr == {"age": 1, "labels": ["Q"]} and g["e"].attr == {}
    assert g.add_nodes([]) == 0 and g.node_count() == 5


def test_add_nodes_checks_everything_first():
    g = Vertex()
    g.add_node("a")
    for bad, err in [
        (["x", "a"], ValueError),  # exists
        (["x", "x"], ValueError),  # twice in the batch
        (["x", 5], TypeError),
        (["x", ("y", 1)], TypeError),  # attrs not a dict
        (["x", ("y", {}, 1)], ValueError),
    ]:
        with pytest.raises(err):
            g.add_nodes(bad)
        assert g.keys() == ["a"]
    with pytest.raises(ValueError, match="columns"):
        g.add_nodes(["x", "y"], attrs={"age": [1]})
    assert g.keys() == ["a"]


def test_add_edges():
    g = Vertex()
    g.add_nodes(["a", "b", "c"])
    d = {"w": 1}
    assert g.add_edges([("a", "b"), ("b", "c", d), ["c", "a", {"type": "t"}]]) == 3
    d["w"] = 99  # the graph keeps its own copy
    edges = {(e.from_node.id, e.to_node.id): e for n in g for e in n.edges}
    assert [e.id for e in edges.values()] == [0, 1, 2]
    assert edges["b", "c"].attr == {"w": 1}
    assert edges["c", "a"].type == "t" and edges["a", "b"].type is None
    # type= for all; columns (None: not set; a "type" column sets types)
    assert g.add_edges([("a", "c"), ("c", "b")], type="k") == 2
    assert g.add_edges([("a", "a"), ("b", "b")], attrs={"weight": [0.5, None], "type": ["x", None]}) == 2
    a_loop = [e for e in g["a"].edges if e.to_node.id == "a"][0]
    b_loop = [e for e in g["b"].edges if e.to_node.id == "b"][0]
    assert (a_loop.attr, a_loop.type) == ({"weight": 0.5, "type": "x"}, "x")
    assert (b_loop.attr, b_loop.type) == ({}, None)
    assert g.project(weight="weight", default_weight=1.0).edge_count() == 7


def test_add_edges_checks_everything_first():
    g = Vertex()
    g.add_nodes(["a", "b"])
    for bad, err in [
        ([("a", "b"), ("a", "zz")], ValueError),
        ([("a", "b"), ("a",)], ValueError),
        ([("a", "b"), "ab"], TypeError),
        ([("a", "b"), ("a", 1)], TypeError),
        ([("a", "b", 5)], TypeError),
    ]:
        with pytest.raises(err):
            g.add_edges(bad)
        assert g.project().edge_count() == 0
    with pytest.raises(ValueError, match="columns"):
        g.add_edges([("a", "b")], attrs={"w": [1, 2]})
    with pytest.raises(ValueError, match="same length"):
        g.add_edges([("a", "b")], attrs={"w": [1], "v": [1, 2]})
    assert g.project().edge_count() == 0


def test_callbacks_fire_after_the_batch():
    g = Vertex()
    seen = []
    g.on_node_add_callbacks.append(lambda v, n: seen.append(("node", n.id, v.node_count())))
    g.on_edge_add_callbacks.append(lambda v, e: seen.append(("edge", e.id)))
    g.add_nodes(["a", "b"])
    g.add_edges([("a", "b"), ("b", "a")])
    assert seen == [("node", "a", 2), ("node", "b", 2), ("edge", 0), ("edge", 1)]


def test_generators_and_large_batches():
    g = Vertex()
    g.add_nodes(str(i) for i in range(1000))
    n = g.add_edges(((str(i), str((i * 7) % 1000)) for i in range(1000)), attrs={"i": list(range(1000))})
    assert n == 1000
    p = g.project()
    assert p.edge_count() == 1000 and p.neighbors("1") == ["7"]
    assert g["3"].edges[0].attr == {"i": 3}
