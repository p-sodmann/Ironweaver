import os
import sys

ROOT = os.path.dirname(os.path.dirname(__file__))
PYTHON_DIR = os.path.join(ROOT, "python")
sys.path.insert(0, PYTHON_DIR)

try:  # pragma: no cover - optional build step
    from ironweaver import Vertex, NodeView
    from ironweaver.filter.predicates import (
        attr_contains,
        attr_equals,
        p_and,
        p_not,
        p_or,
    )
except Exception as e:  # pragma: no cover - optional build step
    import pytest
    pytest.skip(f"ironweaver module unavailable: {e}", allow_module_level=True)


def build_graph():
    v = Vertex()
    v.add_node("n1", {"type": "field", "Labels": ["Field"]})
    v.add_node("n2", {"type": "selector", "Labels": ["Selector"]})
    v.add_node("n3", {"type": "other", "Labels": ["Other"]})
    v.add_edge("n1", "n2", {})
    v.add_edge("n2", "n3", {})
    return v


def build_rich_graph():
    """Build a graph with varied attributes for lambda filtering tests."""
    v = Vertex()
    v.add_node("test_a", {"type": "A", "score": 0.5, "status": "active", "tags": ["important"]})
    v.add_node("test_b", {"type": "B", "score": 0.9, "status": "archived", "tags": ["low"]})
    v.add_node("test_c", {"type": "C", "score": 0.3, "status": "active", "tags": ["important", "urgent"]})
    v.add_node("other_d", {"type": "A", "score": 0.7, "status": "active", "tags": []})
    v.add_node("other_e", {"type": "D", "score": 0.1, "status": "draft", "tags": ["low"]})
    v.add_edge("test_a", "test_b", {"weight": 1.0})
    v.add_edge("test_b", "test_c", {"weight": 2.0})
    v.add_edge("test_c", "other_d", {"weight": 0.5})
    v.add_edge("other_d", "other_e", {"weight": 1.5})
    return v


# ---- Existing predicate-helper tests ----

def test_attr_equals():
    v = build_graph()
    nodes = list(v.filter(attr_equals("type", "selector")))
    assert {n.id for n in nodes} == {"n2"}


def test_attr_contains_or():
    v = build_graph()
    pred = p_or(attr_contains("Labels", "Field"), attr_equals("type", "selector"))
    nodes = list(v.filter(pred))
    assert {n.id for n in nodes} == {"n1", "n2"}


def test_combinators():
    v = build_graph()
    pred = p_and(attr_contains("Labels", "Field"), p_not(attr_equals("type", "selector")))
    nodes = list(v.filter(pred))
    assert {n.id for n in nodes} == {"n1"}


# ---- Lambda / NodeView filtering tests ----

def test_lambda_filter_by_id_startswith():
    v = build_rich_graph()
    result = v.filter(lambda n: n.id.startswith("test_"))
    assert {n.id for n in result} == {"test_a", "test_b", "test_c"}


def test_lambda_filter_by_type():
    v = build_rich_graph()
    result = v.filter(lambda n: n.type in {"A", "B"})
    assert {n.id for n in result} == {"test_a", "test_b", "other_d"}


def test_lambda_filter_by_attr():
    v = build_rich_graph()
    result = v.filter(lambda n: n.attr("score") < 0.8)
    assert {n.id for n in result} == {"test_a", "test_c", "other_d", "other_e"}


def test_lambda_filter_combined():
    """The exact example from the user request."""
    v = build_rich_graph()
    result = v.filter(lambda n: (
        n.id.startswith("test_")
        and n.type in {"A", "B", "C"}
        and n.attr("score") < 0.8
        and n.attr("status") != "archived"
    ))
    assert {n.id for n in result} == {"test_a", "test_c"}


def test_lambda_filter_attr_default():
    v = Vertex()
    v.add_node("n1", {"score": 0.5})
    v.add_node("n2", {})
    result = v.filter(lambda n: n.attr("score", 0.0) > 0.3)
    assert {n.id for n in result} == {"n1"}


def test_lambda_filter_has_attr():
    v = build_rich_graph()
    result = v.filter(lambda n: n.has_attr("tags"))
    assert {n.id for n in result} == {"test_a", "test_b", "test_c", "other_d", "other_e"}


def test_lambda_filter_no_match_returns_empty_vertex():
    v = build_rich_graph()
    result = v.filter(lambda n: n.id == "nonexistent")
    assert len(list(result)) == 0


def test_lambda_filter_preserves_edges():
    """Filtered Vertex should keep edges between included nodes."""
    v = build_rich_graph()
    result = v.filter(lambda n: n.id in {"test_a", "test_b"})
    # test_a -> test_b edge should be preserved
    nodes = {n.id: n for n in result}
    a_edges = nodes["test_a"].edges
    assert len(a_edges) == 1
    assert a_edges[0].to_node.id == "test_b"
    # test_b -> test_c should be removed (test_c not in result)
    b_edges = nodes["test_b"].edges
    assert len(b_edges) == 0


def test_lambda_filter_result_is_vertex():
    """Filter result should be a Vertex with standard capabilities."""
    v = build_rich_graph()
    result = v.filter(lambda n: n.type == "A")
    assert hasattr(result, "nodes")
    assert hasattr(result, "has_node")


def test_nodeview_degree():
    v = build_rich_graph()
    result = v.filter(lambda n: n.degree > 0)
    # All nodes with outgoing edges
    ids = {n.id for n in result}
    assert "test_a" in ids
    assert "other_e" not in ids  # last node, no outgoing edges


def test_nodeview_neighbor_ids():
    v = build_rich_graph()
    result = v.filter(lambda n: "test_b" in n.neighbor_ids)
    assert {n.id for n in result} == {"test_a"}


def test_nodeview_has_edge_to():
    v = build_rich_graph()
    result = v.filter(lambda n: n.has_edge_to("test_c"))
    assert {n.id for n in result} == {"test_b"}


def test_nodeview_attrs_dict():
    v = build_rich_graph()
    result = v.filter(lambda n: "score" in n.attrs and n.attrs["score"] < 0.5)
    assert {n.id for n in result} == {"test_c", "other_e"}


def test_vertex_len():
    v = build_rich_graph()
    assert len(v) == 5
    result = v.filter(lambda n: n.type == "A")
    assert len(result) == 2


def test_vertex_iter():
    v = build_graph()
    ids = {n.id for n in v}
    assert ids == {"n1", "n2", "n3"}


def test_filter_no_args_raises():
    v = build_graph()
    import pytest
    with pytest.raises(ValueError):
        v.filter()
