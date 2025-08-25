import os
import sys

ROOT = os.path.dirname(os.path.dirname(__file__))
PYTHON_DIR = os.path.join(ROOT, "python")
sys.path.insert(0, PYTHON_DIR)

try:
    from ironweaver import Vertex
except Exception as e:  # pragma: no cover - optional build step
    import pytest
    pytest.skip(f"ironweaver module unavailable: {e}", allow_module_level=True)


def build_graph():
    v = Vertex()
    v.add_node("n1", {"attribute": "is_this"})
    v.add_node("n2", {"attribute": "other"})
    v.add_node("n3", {"attribute": "is_this"})
    v.add_edge("n1", "n2", {})
    v.add_edge("n2", "n3", {})
    return v


def test_filter_ids():
    v = build_graph()
    filtered = v.filter(ids=["n1", "n3"])
    assert set(filtered.keys()) == {"n1", "n3"}


def test_filter_id_single():
    v = build_graph()
    filtered = v.filter(id="n2")
    assert set(filtered.keys()) == {"n2"}


def test_filter_attribute():
    v = build_graph()
    filtered = v.filter(attribute="is_this")
    assert set(filtered.keys()) == {"n1", "n3"}
