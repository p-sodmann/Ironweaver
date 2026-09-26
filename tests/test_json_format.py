"""Lock down the on-disk graph format (JSON and bincode) and its options."""

import json
import os
import re

import pytest

try:
    from ironweaver import Vertex
except Exception as e:  # pragma: no cover - module unavailable
    pytest.skip(f"ironweaver module unavailable: {e}", allow_module_level=True)

DATA = os.path.join(os.path.dirname(os.path.abspath(__file__)), "data")
EDGE_ID = re.compile(r"^edge_(\d+)_(.+)_to_(.+)$")


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
    """Replace run-dependent edge ids by (from, to, attr) and drop the timestamp."""
    edges = doc["edges"]
    key = {eid: (e["from_id"], e["to_id"], json.dumps(e["attr"], sort_keys=True)) for eid, e in edges.items()}
    for eid, e in edges.items():
        m = EDGE_ID.match(eid)
        assert m and e["id"] == eid and (m.group(2), m.group(3)) == (e["from_id"], e["to_id"])
    counters = sorted(int(EDGE_ID.match(eid).group(1)) for eid in edges)
    assert counters == list(range(len(edges)))
    nodes = {
        nid: {
            **{k: v for k, v in n.items() if k not in ("edge_ids", "inverse_edge_ids")},
            "edge_ids": [key[e] for e in n["edge_ids"]],
            "inverse_edge_ids": sorted(key[e] for e in n["inverse_edge_ids"]),
        }
        for nid, n in doc["nodes"].items()
    }
    metadata = {k: v for k, v in doc["metadata"].items() if k != "timestamp"}
    assert "String" in doc["metadata"]["timestamp"]
    return {
        "nodes": nodes,
        "edges": sorted(
            (key[eid], json.dumps(e["meta"], sort_keys=True)) for eid, e in edges.items()
        ),
        "meta": doc["meta"],
        "metadata": metadata,
    }


def test_json_document_shape_is_stable():
    doc = json.loads(sample_graph().save_to_json())
    with open(os.path.join(DATA, "legacy_graph.json"), encoding="utf-8") as fh:
        legacy = json.load(fh)
    assert canonical(doc) == canonical(legacy)
    # Spot-check the tagged value encoding explicitly
    attr = doc["nodes"]["a"]["attr"]
    assert attr["score"] == {"Float": 1.5}
    assert attr["flag"] == {"Bool": True}
    assert attr["tags"] == {"List": [{"String": "x"}, {"Int": 2}, "None"]}
    assert attr["nested"] == {"Dict": {"k": {"Int": 3}}}
    assert doc["meta"] == {"title": {"String": "legacy"}}


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
    assert sorted((e.to_node.id, e.attr.get("type")) for e in a.edges) == [("b", "knows"), ("c", "likes")]
    assert [e.from_node.id for e in a.inverse_edges] == ["b"]
    assert v.meta["title"] == "legacy"
    assert a.vertex is v


@pytest.mark.parametrize("name", ["legacy_graph.json", "legacy_graph.bin", "legacy_graph_f16.bin"])
def test_legacy_files_still_load(name):
    path = os.path.join(DATA, name)
    v = Vertex.load_from_json(path) if name.endswith(".json") else Vertex.load_from_binary(path)
    check_loaded(v)


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
