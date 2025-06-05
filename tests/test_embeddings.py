import os
import sys

ROOT = os.path.dirname(os.path.dirname(__file__))
sys.path.insert(0, ROOT)

try:
    from ironweaver import Vertex
except Exception as e:  # pragma: no cover - optional build step
    import pytest
    pytest.skip(f"ironweaver module unavailable: {e}", allow_module_level=True)

from embedding_utils import attach_embeddings_from_meta


def add_embeddings(vertex, node):
    if "embedding" not in vertex.meta:
        vertex.meta["embedding"] = []
        vertex.meta["embedding_ids"] = []
    if "embedding" in node.attr:
        vertex.meta["embedding"].append(node.attr["embedding"])
        vertex.meta["embedding_ids"].append(node.id)
    return True


def test_attach_embeddings_from_meta():
    v = Vertex()
    v.on_node_add_callbacks.append(add_embeddings)
    v.add_node("node1", {"embedding": [1, 2, 3]})
    v.add_node("node2", {"embedding": [2, 1, 3]})

    attach_embeddings_from_meta(v)

    assert v.get_node("node1").attr["embeddings"] == [[1, 2, 3]]
    assert v.get_node("node2").attr["embeddings"] == [[2, 1, 3]]


def test_attr_helpers():
    v = Vertex()
    node = v.add_node("n", {})

    node.attr_set("foo", 1)
    assert node.attr_get("foo") == 1

    node.attr_list_append("bar", 5)
    node.attr_list_append("bar", 6)
    assert node.attr_get("bar") == [5, 6]
