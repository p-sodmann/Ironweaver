"""Lock down the on-disk graph format (JSON and binary) and its options."""

import json
import os
import re

import pytest

try:
    from ironweaver import Vertex
except Exception as e:  # pragma: no cover - module unavailable
    pytest.skip(f"ironweaver module unavailable: {e}", allow_module_level=True)

DATA = os.path.join(os.path.dirname(os.path.abspath(__file__)), "data")


def sample_graph():
    # Same graph as the one that produced tests/data/legacy_graph.*
    g = Vertex()
    g.add_node("a", {"name": "alpha", "score": 1.5, "flag": True, "tags": ["x", 2, None], "nested": {"k": 3}})
    g.add_node("b", {"name": "beta"})
    g.add_node("c")
    g.get_node("a").meta = {"note": "m"}
    g.add_edge("a", "b", {"type": "knows", "weight": 0.25})
    g.add_edge("a", "c", {"type": "likes"})
    g.add_edge("b", "a")
    g.meta["title"] = "legacy"
    return g


def canonical(doc):
    """The document without its timestamp (edge ids are stable in v2)."""
    doc = json.loads(json.dumps(doc))
    assert "String" in doc["metadata"].pop("timestamp")
    return doc


def test_json_document_shape_is_stable():
    g = sample_graph()
    g["a"].add_label("Person")
    doc = json.loads(g.save_to_json())
    assert canonical(doc) == {
        "nodes": {
            "a": {
                "id": "a",
                "labels": ["Person"],
                "attr": {
                    "name": {"String": "alpha"},
                    "score": {"Float": 1.5},
                    "flag": {"Bool": True},
                    "tags": {"List": [{"String": "x"}, {"Int": 2}, "None"]},
                    "nested": {"Dict": {"k": {"Int": 3}}},
                },
                "meta": {"note": {"String": "m"}},
                "edge_ids": ["0", "1"],
                "inverse_edge_ids": ["2"],
            },
            "b": {"id": "b", "labels": [], "attr": {"name": {"String": "beta"}}, "meta": {},
                  "edge_ids": ["2"], "inverse_edge_ids": ["0"]},
            "c": {"id": "c", "labels": [], "attr": {}, "meta": {}, "edge_ids": [], "inverse_edge_ids": ["1"]},
        },
        "edges": {
            "0": {"id": "0", "from_id": "a", "to_id": "b", "type": "knows",
                  "attr": {"weight": {"Float": 0.25}}, "meta": {}},
            "1": {"id": "1", "from_id": "a", "to_id": "c", "type": "likes", "attr": {}, "meta": {}},
            "2": {"id": "2", "from_id": "b", "to_id": "a", "type": None, "attr": {}, "meta": {}},
        },
        "meta": {"title": {"String": "legacy"}},
        "metadata": {
            "version": {"String": "2.0"},
            "node_count": {"Int": 3},
            "edge_count": {"Int": 3},
            "next_edge_id": {"String": "3"},
        },
    }


def test_edge_ids_labels_and_types_survive_round_trips(tmp_path):
    g = sample_graph()
    g["b"].labels = ["X", "Y"]
    removed = g["b"].edges[0].id
    g.remove_edge("b", "a")
    e = g.add_edge("c", "b", type="t")
    path = str(tmp_path / "g.bin")
    g.save_to_binary(path)
    for loaded in (Vertex.load_from_json(g.save_to_json()), Vertex.load_from_binary(path)):
        assert loaded.get_edge(e.id).type == "t"
        assert loaded.get_edge(0).to_node.id == "b"
        assert loaded["b"].labels == ["X", "Y"]
        assert [n.id for n in loaded.nodes_with_label("Y")] == ["b"]
        with pytest.raises(KeyError):
            loaded.get_edge(removed)
        # Ids of removed edges are not handed out again
        assert loaded.add_edge("a", "a").id == e.id + 1


def test_compact_by_default_and_pretty_option(tmp_path):
    g = sample_graph()
    compact = g.save_to_json()
    pretty = g.save_to_json(pretty=True)
    assert "\n" not in compact
    assert pretty.startswith("{\n  ")
    assert canonical(json.loads(compact)) == canonical(json.loads(pretty))

    path = os.path.join(tmp_path, "g.json")
    g.save_to_json(path)
    with open(path, encoding="utf-8") as fh:
        assert "\n" not in fh.read()
    g.save_to_json(path, pretty=True)
    with open(path, encoding="utf-8") as fh:
        assert fh.read().startswith("{\n  ")


def check_loaded(v):
    assert sorted(v.keys()) == ["a", "b", "c"]
    a = v["a"]
    assert a.attr == {"name": "alpha", "score": 1.5, "flag": True, "tags": ["x", 2, None], "nested": {"k": 3}}
    assert a.meta == {"note": "m"}
    assert sorted((e.to_node.id, e.type) for e in a.edges) == [("b", "knows"), ("c", "likes")]
    assert sorted((e.to_node.id, e.attr.get("type")) for e in a.edges) == [("b", "knows"), ("c", "likes")]
    assert sorted(e.id for n in v for e in n.edges) == [0, 1, 2]
    assert [e.from_node.id for e in a.inverse_edges] == ["b"]
    assert v.meta["title"] == "legacy"
    assert a.vertex is v


def test_legacy_json_still_loads():
    v = Vertex.load_from_json(os.path.join(DATA, "legacy_graph.json"))
    check_loaded(v)
    # Version 1 files: old string edge ids are kept in meta, edges get new
    # integer ids, and attr "type" became the edge type
    a = v["a"]
    assert sorted(e.meta["legacy_id"] for e in a.edges) == ["edge_0_a_to_b", "edge_1_a_to_c"]
    for e in a.edges:  # new ids are handed out in the file's edge order
        assert v.get_edge(e.id) == e



@pytest.mark.parametrize("name", ["legacy_graph.bin", "legacy_graph_f16.bin"])
def test_legacy_binary_files_are_refused(name):
    with pytest.raises(RuntimeError, match="unsupported ironweaver binary format version 1") as e:
        Vertex.load_from_binary(os.path.join(DATA, name))
    assert "save_to_json" in str(e.value)


def test_files_without_header_are_not_taken_for_legacy(tmp_path):
    for data in [b"{}", b"\xff" * 64, open(os.path.join(DATA, "legacy_graph.json"), "rb").read()]:
        path = tmp_path / "g.bin"
        path.write_bytes(data)
        with pytest.raises(RuntimeError, match="invalid ironweaver binary file") as e:
            Vertex.load_from_binary(str(path))
        assert "version 1" not in str(e.value)

def test_round_trips(tmp_path):
    g = sample_graph()
    check_loaded(Vertex.load_from_json(g.save_to_json()))
    check_loaded(Vertex.load_from_json(g.save_to_json(pretty=True)))
    check_loaded(Vertex.load_from_json(json.loads(g.save_to_json())))
    for method in ("save_to_json", "save_to_binary", "save_to_binary_f16"):
        path = os.path.join(tmp_path, method)
        getattr(g, method)(path)
        loaded = Vertex.load_from_json(path) if method == "save_to_json" else Vertex.load_from_binary(path)
        check_loaded(loaded)


def test_json_string_with_leading_whitespace():
    check_loaded(Vertex.load_from_json("  \n" + sample_graph().save_to_json()))


def test_unserializable_value_raises_runtime_error(tmp_path):
    class Broken:
        def __str__(self):
            raise ValueError("cannot stringify")

    g = Vertex()
    g.add_node("a", {"bad": Broken()})
    with pytest.raises(RuntimeError, match="cannot stringify"):
        g.save_to_json()
    with pytest.raises(RuntimeError, match="cannot stringify"):
        g.save_to_binary(os.path.join(tmp_path, "g.bin"))


def test_missing_file_raises_runtime_error(tmp_path):
    with pytest.raises(RuntimeError):
        Vertex.load_from_json(os.path.join(tmp_path, "nope.json"))


@pytest.mark.parametrize("name", ["v2_graph.json", "v2_graph.bin", "v2_graph_f16.bin"])
def test_v2_files_still_load(name):
    """Golden files written by format version 2: later versions must keep
    reading them with ids, labels and types intact."""
    path = os.path.join(DATA, name)
    v = Vertex.load_from_json(path) if name.endswith(".json") else Vertex.load_from_binary(path)
    assert sorted(v.keys()) == ["a", "b", "c"]
    a = v["a"]
    assert a.labels == ["Person"] and a.meta == {"note": "m"}
    assert a.attr_get("nested") == {"k": 3} and a.attr_get("tags") == ["x", 2, None]
    edges = sorted((e.id, e.from_node.id, e.to_node.id, e.type) for n in v for e in n.edges)
    assert edges == [(0, "a", "b", "knows"), (1, "a", "c", "likes"), (3, "c", "a", "back")]
    assert v.get_edge(0).attr_get("weight") == 0.25
    assert v.add_edge("a", "a").id == 4   # id 2 was removed before saving: not reused
    assert v.meta["title"] == "legacy"

