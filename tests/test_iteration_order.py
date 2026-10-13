"""Seeded iteration must expose accidental reliance on graph order."""
import os
import subprocess
import sys

import pytest

from ironweaver import Vertex, attr


def graph(seed=None, ids=None):
    g = Vertex(shuffle_seed=seed)
    ids = list(ids) if ids is not None else [f"n{i}" for i in range(32)]
    g.add_nodes(ids, labels=["Item"], attrs={"value": [1] * len(ids)})
    return g


def test_default_and_seeded_views():
    default = graph()
    g = graph(1234)
    order = g.keys()
    assert order != default.keys()
    assert sorted(order) == sorted(default.keys())
    assert order == graph(1234).keys() == graph(1234, reversed(default.keys())).keys()
    assert order != graph(4321).keys()
    assert list(g.nodes) == list(g.toJSON()) == order
    assert [n.id for n in g] == order
    assert repr(g) == f"Vertex({', '.join(order)})"
    assert g.project().ids() == order
    g.set_shuffle_seed()
    assert g.shuffle_seed is None
    assert g.keys() == default.keys()
    g.set_shuffle_seed(1234)
    assert g.keys() == order


def test_queries_and_subgraphs():
    g = graph(1234)
    order = g.keys()
    assert [n.id for n in g.nodes_with_label("Item")] == order
    assert [n.id for n in g.find("value", 1)] == order
    g.create_index("value")
    assert [n.id for n in g.find("value", 1)] == order
    assert [n.id for n in g.find_range("value", 0, 2)] == order
    assert [row["n"].id for row in g.match("(n:Item)")] == order
    assert [row["n"].id for row in g.match("(n)", ids={"n": order})] == order
    for sub in [g.filter(attr("value") == 1), g.filter(lambda n: n.attr("value") == 1)]:
        assert sub.shuffle_seed == 1234
        assert sub.keys() == order
    empty = g.filter(lambda n: False)
    assert empty.shuffle_seed == 1234
    assert empty.keys() == []
    g.add_edge(order[0], order[1])
    sub = g[order[0]].bfs(depth=1)
    assert sub.shuffle_seed == 1234
    assert sub.keys() == order[:2]


def test_mutation_and_small_graphs():
    g = graph(0)
    g.remove_node("n4")
    g.add_node("replacement")
    expected = graph(0, [n for n in graph().keys() if n != "n4"] + ["replacement"])
    assert g.keys() == expected.keys()
    assert Vertex(shuffle_seed=0).keys() == []
    one = Vertex(shuffle_seed=2**64 - 1)
    one.add_node("only")
    assert one.keys() == ["only"]
    with pytest.raises(OverflowError):
        Vertex(shuffle_seed=-1)
    with pytest.raises(OverflowError):
        g.set_shuffle_seed(2**64)
    with pytest.raises(TypeError):
        Vertex(shuffle_seed="123")


def test_reproducible_across_processes():
    code = "from ironweaver import Vertex; g=Vertex(shuffle_seed=1234); g.add_nodes([f'n{i}' for i in range(32)]); print(','.join(g.keys()))"
    orders = [subprocess.check_output([sys.executable, "-c", code], env={**os.environ, "PYTHONHASHSEED": str(seed)}, text=True) for seed in [1, 42]]
    assert orders[0] == orders[1] == ",".join(graph(1234).keys()) + "\n"


def test_seed_is_runtime_only(tmp_path):
    g = graph(1234)
    path = str(tmp_path / "graph.bin")
    g.save_to_binary(path)
    for loaded in [Vertex.load_from_json(g.save_to_json()), Vertex.load_from_binary(path)]:
        assert loaded.shuffle_seed is None
        loaded.set_shuffle_seed(1234)
        assert loaded.keys() == g.keys()


def test_subset_queries_and_dirty_indexes():
    g = graph(1234)
    order = g.keys()
    selected = set(order[::3])
    for id in selected:
        g[id].add_label("Selected")
        g[id].attr_set("value", 2)
    expected = [id for id in order if id in selected]
    assert [n.id for n in g.nodes_with_label("Selected")] == expected
    assert [row["n"].id for row in g.match("(n:Selected)")] == expected
    g.create_index("value")
    assert [row["n"].id for row in g.match("(n {value: 2})")] == expected
    assert [n.id for n in g.find("value", 2)] == expected
    assert g.filter((attr("value") == 1) | (attr("value") == 2)).keys() == order
    g[order[0]].attr_set("value", 3)
    assert [n.id for n in g.find_range("value", 1, 3)] == order
