import json
import os
import sys

import pytest

ROOT = os.path.dirname(os.path.dirname(__file__))
PYTHON_DIR = os.path.join(ROOT, "python")
sys.path.insert(0, PYTHON_DIR)

try:
    from ironweaver import Vertex
except Exception as e:  # pragma: no cover - optional build step
    pytest.skip(f"ironweaver module unavailable: {e}", allow_module_level=True)


def linear_graph():
    """a -> b -> c -> d"""
    v = Vertex()
    v.add_node("a", {"type": "start"})
    v.add_node("b", {"type": "mid"})
    v.add_node("c", {"type": "mid"})
    v.add_node("d", {"type": "end"})
    v.add_edge("a", "b", {})
    v.add_edge("b", "c", {})
    v.add_edge("c", "d", {})
    return v


# ---- get_metadata ----

def test_get_metadata_counts():
    v = linear_graph()
    meta = v.get_metadata()
    assert meta["node_count"] == 4
    assert meta["edge_count"] == 3


def test_get_metadata_returns_dict():
    v = Vertex()
    v.add_node("x", {})
    meta = v.get_metadata()
    assert isinstance(meta, dict)


def test_get_metadata_empty_graph():
    v = Vertex()
    meta = v.get_metadata()
    assert meta["node_count"] == 0
    assert meta["edge_count"] == 0


# ---- save_to_json (no file_path → returns JSON string) ----

def test_save_to_json_no_path_returns_string():
    v = Vertex()
    v.add_node("n1", {"val": 1})
    result = v.save_to_json()
    assert isinstance(result, str)


def test_save_to_json_no_path_is_valid_json():
    v = Vertex()
    v.add_node("n1", {"val": 1})
    result = v.save_to_json()
    data = json.loads(result)
    assert "n1" in data["nodes"]


def test_save_to_json_roundtrip():
    v = Vertex()
    v.add_node("a", {"x": 42})
    v.add_node("b", {})
    v.add_edge("a", "b", {"w": 1})
    s = v.save_to_json()
    v2 = Vertex.load_from_json(s)
    assert v2.has_node("a")
    assert v2.has_node("b")
    assert v2.get_node("a").attr.get("x") == 42


# ---- prune ----

def test_prune_returns_int():
    v = linear_graph()
    count = v.prune()
    assert isinstance(count, int)


def test_prune_no_dangling_edges_returns_zero():
    v = linear_graph()
    count = v.prune()
    assert count == 0


def test_prune_removes_edges_to_absent_nodes():
    """from_nodes copies node objects (with their edges intact), so b->c is dangling
    when only {a, b} are in the new Vertex."""
    large = linear_graph()
    a = large.get_node("a")
    b = large.get_node("b")
    sub = Vertex.from_nodes({"a": a, "b": b})
    # a->b is internal; b->c is dangling (c absent from sub)
    count = sub.prune()
    assert count > 0
    for node in sub:
        for edge in node.edges:
            assert sub.has_node(edge.to_node.id), f"dangling edge to {edge.to_node.id}"


# ---- shortest_path_bfs ----

def test_shortest_path_bfs_includes_endpoints():
    v = linear_graph()
    result = v.shortest_path_bfs("a", "d")
    ids = {n.id for n in result}
    assert "a" in ids
    assert "d" in ids


def test_shortest_path_bfs_includes_intermediates():
    v = linear_graph()
    result = v.shortest_path_bfs("a", "d")
    ids = {n.id for n in result}
    assert ids == {"a", "b", "c", "d"}


def test_shortest_path_bfs_adjacent_nodes():
    v = linear_graph()
    result = v.shortest_path_bfs("a", "b")
    ids = {n.id for n in result}
    assert "a" in ids
    assert "b" in ids


def test_shortest_path_bfs_returns_vertex():
    v = linear_graph()
    result = v.shortest_path_bfs("a", "d")
    assert hasattr(result, "nodes")
    assert hasattr(result, "has_node")


# ---- expand ----

def test_expand_adds_neighbours():
    large = linear_graph()  # a->b->c->d
    small = large.filter(ids=["a"])
    result = small.expand(large, depth=1)
    ids = {n.id for n in result}
    assert "a" in ids
    assert "b" in ids


def test_expand_returns_vertex():
    large = linear_graph()
    small = large.filter(ids=["b"])
    result = small.expand(large, depth=1)
    assert hasattr(result, "nodes")
    assert hasattr(result, "has_node")


def test_expand_depth_zero_returns_self():
    large = linear_graph()
    small = large.filter(ids=["a"])
    result = small.expand(large, depth=0)
    ids = {n.id for n in result}
    assert "a" in ids
    assert "b" not in ids


# ---- to_networkx ----

def test_to_networkx_nodes():
    nx = pytest.importorskip("networkx")
    v = linear_graph()
    g = v.to_networkx()
    assert set(g.nodes()) == {"a", "b", "c", "d"}


def test_to_networkx_edges():
    nx = pytest.importorskip("networkx")
    v = linear_graph()
    g = v.to_networkx()
    assert g.has_edge("a", "b")
    assert g.has_edge("b", "c")
    assert g.has_edge("c", "d")


def test_to_networkx_is_digraph():
    nx = pytest.importorskip("networkx")
    v = linear_graph()
    g = v.to_networkx()
    assert isinstance(g, nx.DiGraph)


def test_to_networkx_empty():
    pytest.importorskip("networkx")
    v = Vertex()
    g = v.to_networkx()
    assert g is not None


# ---- on_edge_add_callbacks ----

def test_on_edge_add_callback_fires():
    calls = []

    def cb(vertex, edge):
        calls.append((edge.from_node.id, edge.to_node.id))
        return True

    v = Vertex()
    v.on_edge_add_callbacks.append(cb)
    v.add_node("x", {})
    v.add_node("y", {})
    assert len(calls) == 0
    v.add_edge("x", "y", {})
    assert len(calls) == 1
    assert calls[0] == ("x", "y")


def test_on_edge_add_callback_multiple_edges():
    calls = []

    def cb(vertex, edge):
        calls.append(edge.from_node.id)
        return True

    v = Vertex()
    v.on_edge_add_callbacks.append(cb)
    v.add_node("a", {})
    v.add_node("b", {})
    v.add_node("c", {})
    v.add_edge("a", "b", {})
    v.add_edge("b", "c", {})
    assert calls == ["a", "b"]


def test_on_edge_add_callback_not_called_for_nodes():
    calls = []

    def cb(vertex, edge):
        calls.append(edge)
        return True

    v = Vertex()
    v.on_edge_add_callbacks.append(cb)
    v.add_node("n", {})
    assert len(calls) == 0


# ---- on_node_update_callbacks ----

def test_on_node_update_callback_fires():
    calls = []

    def cb(vertex, node, key, new_val, old_val):
        calls.append((node.id, key, new_val, old_val))
        return True

    v = Vertex()
    v.on_node_update_callbacks.append(cb)
    node = v.add_node("n", {})
    node.attr_set("score", 0.5)

    assert len(calls) == 1
    assert calls[0] == ("n", "score", 0.5, None)


def test_on_node_update_callback_old_value_passed():
    calls = []

    def cb(vertex, node, key, new_val, old_val):
        calls.append((new_val, old_val))
        return True

    v = Vertex()
    v.on_node_update_callbacks.append(cb)
    node = v.add_node("n", {})
    node.attr_set("x", 1)
    node.attr_set("x", 2)

    assert calls[1] == (2, 1)


def test_on_node_update_callback_no_fire_same_value():
    calls = []

    def cb(vertex, node, key, new_val, old_val):
        calls.append(new_val)
        return True

    v = Vertex()
    v.on_node_update_callbacks.append(cb)
    node = v.add_node("n", {})
    node.attr_set("x", 1)
    node.attr_set("x", 1)  # same value — should not fire again

    assert len(calls) == 1


# ---- on_edge_update_callbacks ----

def test_on_edge_update_callback_fires():
    calls = []

    def cb(vertex, edge, key, new_val, old_val):
        calls.append((key, new_val, old_val))
        return True

    v = Vertex()
    v.on_edge_update_callbacks.append(cb)
    v.add_node("a", {})
    v.add_node("b", {})
    edge = v.add_edge("a", "b", {})
    edge.attr_set("weight", 2.0)

    assert len(calls) == 1
    assert calls[0] == ("weight", 2.0, None)


def test_on_edge_update_callback_old_value():
    calls = []

    def cb(vertex, edge, key, new_val, old_val):
        calls.append((new_val, old_val))
        return True

    v = Vertex()
    v.on_edge_update_callbacks.append(cb)
    v.add_node("a", {})
    v.add_node("b", {})
    edge = v.add_edge("a", "b", {"weight": 1.0})
    edge.attr_set("weight", 3.0)

    assert len(calls) == 1
    assert calls[0] == (3.0, 1.0)


# ---- Node.vertex back-reference ----

def test_node_vertex_backref():
    v = Vertex()
    node = v.add_node("n", {})
    assert node.vertex is v


def test_node_vertex_backref_multiple_nodes():
    v = Vertex()
    n1 = v.add_node("n1", {})
    n2 = v.add_node("n2", {})
    assert n1.vertex is v
    assert n2.vertex is v


def test_node_vertex_backref_independent_graphs():
    v1 = Vertex()
    v2 = Vertex()
    node1 = v1.add_node("a", {})
    node2 = v2.add_node("a", {})
    assert node1.vertex is v1
    assert node2.vertex is v2
    assert node1.vertex is not v2


# ---- Edge.attr_set / Edge.attr_get ----

def test_edge_attr_set_get():
    v = Vertex()
    v.add_node("a", {})
    v.add_node("b", {})
    edge = v.add_edge("a", "b", {})
    edge.attr_set("weight", 1.5)
    assert edge.attr_get("weight") == 1.5


def test_edge_attr_get_missing_returns_none():
    v = Vertex()
    v.add_node("a", {})
    v.add_node("b", {})
    edge = v.add_edge("a", "b", {})
    assert edge.attr_get("nonexistent") is None


def test_edge_attr_set_updates_attr_dict():
    v = Vertex()
    v.add_node("a", {})
    v.add_node("b", {})
    edge = v.add_edge("a", "b", {})
    edge.attr_set("label", "connects")
    assert edge.attr["label"] == "connects"


def test_edge_attr_set_overwrites():
    v = Vertex()
    v.add_node("a", {})
    v.add_node("b", {})
    edge = v.add_edge("a", "b", {"weight": 1.0})
    edge.attr_set("weight", 9.9)
    assert edge.attr_get("weight") == 9.9


def test_edge_attr_set_fires_vertex_update_callback():
    calls = []

    def cb(vertex, edge, key, new_val, old_val):
        calls.append((key, new_val))
        return True

    v = Vertex()
    v.on_edge_update_callbacks.append(cb)
    v.add_node("a", {})
    v.add_node("b", {})
    edge = v.add_edge("a", "b", {})
    edge.attr_set("x", 99)

    assert len(calls) == 1
    assert calls[0] == ("x", 99)
