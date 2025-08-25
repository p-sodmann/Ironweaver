import os
import sys

ROOT = os.path.dirname(os.path.dirname(__file__))
PYTHON_DIR = os.path.join(ROOT, "python")
sys.path.insert(0, PYTHON_DIR)

try:  # pragma: no cover - optional build step
    from ironweaver import Vertex
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
