import os
import sys

ROOT = os.path.dirname(os.path.dirname(__file__))
sys.path.insert(0, ROOT)

try:
    from ironweaver import Vertex
except Exception as e:
    import pytest
    pytest.skip(f"ironweaver module unavailable: {e}", allow_module_level=True)


def build_large_graph(n):
    v = Vertex()
    for i in range(n):
        v.add_node(f"n{i}", {"value": i})
    for i in range(n - 1):
        v.add_edge(f"n{i}", f"n{i+1}", None)
    return v


def test_large_serialization(tmp_path):
    node_count = 1000
    v = build_large_graph(node_count)
    json_file = tmp_path / "graph.json"
    bin_file = tmp_path / "graph.bin"

    v.save_to_json(str(json_file))
    v.save_to_binary(str(bin_file))

    v_json = Vertex.load_from_json(str(json_file))
    assert v_json.node_count() == node_count

    v_bin = Vertex.load_from_binary(str(bin_file))
    assert v_bin.node_count() == node_count

