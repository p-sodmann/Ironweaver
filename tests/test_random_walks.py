import os
import sys

ROOT = os.path.dirname(os.path.dirname(__file__))
sys.path.insert(0, ROOT)

try:
    from ironweaver import Vertex
except Exception as e:  # pragma: no cover - optional build step
    import pytest
    pytest.skip(f"ironweaver module unavailable: {e}", allow_module_level=True)


def build_vertex(edges):
    v = Vertex()
    nodes = {n for edge in edges for n in edge[:2]}
    for n in nodes:
        v.add_node(n, {})
    for a, b, attr in edges:
        v.add_edge(a, b, {"type": attr})
    return v


def test_min_length_filter():
    v = build_vertex([("n1", "n2", "x")])
    walks = v.random_walks(
        "n1", 3, 5, min_length=3, allow_revisit=False, include_edge_types=False, edge_type_field=None
    )
    assert walks == []


def test_allow_revisit():
    edges = [
        ("n1", "n2", "x"),
        ("n2", "n1", "x"),
        ("n2", "n3", "y"),
    ]
    v = build_vertex(edges)
    no_revisit = v.random_walks(
        "n1", 3, 5, min_length=3, allow_revisit=False, include_edge_types=False, edge_type_field=None
    )
    assert no_revisit == [["n1", "n2", "n3"]]

    with_revisit = v.random_walks(
        "n1", 3, 10, min_length=3, allow_revisit=True, include_edge_types=False, edge_type_field=None
    )
    assert sorted(with_revisit) == sorted([["n1", "n2", "n1"], ["n1", "n2", "n3"]])


def test_include_edge_types():
    v = build_vertex([("n1", "n2", "x"), ("n2", "n3", "y")])
    walks = v.random_walks(
        "n1", 3, 5, min_length=3, allow_revisit=False, include_edge_types=True, edge_type_field="type"
    )
    assert walks == [["n1", "x", "n2", "y", "n3"]]

