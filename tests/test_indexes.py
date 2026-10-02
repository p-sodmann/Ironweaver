"""Property indexes: find / find_range / filter(where=) / match give the same
answers with and without an index, through every way of changing nodes."""

import datetime as dt
import random

import pytest

import ironweaver as iw


def ids(nodes):
    return [n.id for n in nodes]


def people():
    g = iw.Vertex()
    g.add_node("ann", {"age": 31, "city": "Oslo", "born": dt.date(1993, 4, 1)}, labels=["Person"])
    g.add_node("bob", {"age": 25.0, "city": "Rome"}, labels=["Person"])
    g.add_node("cat", {"age": 40, "city": "Oslo"}, labels=["Person"])
    g.add_node("dan", {"age": "unknown"}, labels=["Person"])
    g.add_node("acme", {"city": "Oslo"}, labels=["Company"])
    g.add_edge("ann", "acme", type="works_at")
    g.add_edge("cat", "acme", type="works_at")
    return g


def test_find_and_ranges():
    g = people()
    assert g.indexes == []
    unindexed = (ids(g.find("city", "Oslo")), ids(g.find_range("age", 25, 35)))
    assert unindexed == (["ann", "cat", "acme"], ["ann", "bob"])
    assert g.create_index("city") and g.create_index("age")
    assert not g.create_index("age")
    assert g.indexes == ["city", "age"]
    assert ids(g.find("city", "Oslo")) == ["ann", "cat", "acme"]
    assert ids(g.find("age", 25)) == ["bob"]  # 25 == 25.0
    assert ids(g.find("age", 25.5)) == []
    assert ids(g.find_range("age", 25, 35)) == ["ann", "bob"]
    assert ids(g.find_range("age", 25, 31, inclusive="neither")) == []
    assert ids(g.find_range("age", 25, 31, inclusive="right")) == ["ann"]
    assert ids(g.find_range("age", low=32)) == ["cat"]
    assert ids(g.find_range("age", high="v")) == ["dan"]  # strings with strings
    assert ids(g.find_range("born", dt.date(1990, 1, 1), dt.date(2000, 1, 1))) == ["ann"]
    with pytest.raises(ValueError, match="same kind"):
        g.find_range("age", 1, "z")
    with pytest.raises(ValueError, match="low or high"):
        g.find_range("age")
    with pytest.raises(ValueError, match="inclusive"):
        g.find_range("age", 1, inclusive="all")
    with pytest.raises(ValueError, match="labels"):
        g.create_index("labels")
    assert g.drop_index("city") and not g.drop_index("city")
    assert g.indexes == ["age"]


def test_indexes_follow_changes():
    g = people()
    g.create_index("city")
    g.add_node("eve", {"city": "Oslo"})
    g.add_nodes([("fay", {"city": "Rome"}), "gus"], attrs={"city": [None, "Oslo"]})
    g["bob"].attr_set("city", "Oslo")
    g["ann"].attr = {"city": "Paris"}
    g["cat"].attr_set("city", None)
    g.remove_node("acme")
    g["gus"].id = "gus2"
    assert ids(g.find("city", "Oslo")) == ["bob", "eve", "gus2"]
    assert ids(g.find("city", "Paris")) == ["ann"]
    assert ids(g.find("city", "Rome")) == ["fay"]
    # A value that stops being a scalar leaves the index
    g["bob"].attr_set("city", ["Oslo"])
    assert ids(g.find("city", "Oslo")) == ["eve", "gus2"]


def test_filter_and_match_use_indexes_transparently():
    g = people()
    expr = (iw.attr("age") >= 30) & (iw.attr("age") < 41) & (iw.attr("city") == "Oslo")
    plain = sorted(g.filter(where=expr).nodes)
    query = "(p:Person {city: 'Oslo'})-[:works_at]->(c)"
    plain_matches = [(m["p"].id, m["c"].id) for m in g.match(query)]
    g.create_index("age")
    g.create_index("city")
    assert sorted(g.filter(where=expr).nodes) == plain == ["ann", "cat"]
    assert [(m["p"].id, m["c"].id) for m in g.match(query)] == plain_matches == [("ann", "acme"), ("cat", "acme")]
    where = {"p": iw.attr("age") > 35}
    assert [m["p"].id for m in g.match("(p)-->(c)", where=where)] == ["cat"]


def test_randomized_against_scans():
    rng = random.Random(3)
    values = [None, 0, 1, 1.0, 2.5, -3, "a", "b", True, False, dt.date(2020, 1, 1), b"x", [1], float("nan")]
    g = iw.Vertex()
    g.create_index("x")
    reference = iw.Vertex()
    for step in range(400):
        node_ids = sorted(g.nodes)
        op = rng.randrange(5)
        if op == 0 or not node_ids:
            nid = f"n{step}"
            v = rng.choice(values)
            attrs = {} if v is None else {"x": v}
            g.add_node(nid, attrs)
            reference.add_node(nid, attrs)
        elif op == 1:
            nid = rng.choice(node_ids)
            g.remove_node(nid)
            reference.remove_node(nid)
        else:
            nid = rng.choice(node_ids)
            v = rng.choice(values)
            g[nid].attr_set("x", v)
            reference[nid].attr_set("x", v)
        if step % 20 == 0:
            for probe in [0, 1, 2.5, "a", True, dt.date(2020, 1, 1), b"x"]:
                assert ids(g.find("x", probe)) == ids(reference.find("x", probe)), probe
            for lo, hi in [(0, 2), (-5, None), (None, "az"), (False, True)]:
                assert ids(g.find_range("x", lo, hi)) == ids(reference.find_range("x", lo, hi))


def test_memory_usage():
    g = iw.Vertex()
    empty = g.memory_usage()
    assert empty == g.memory_usage(deep=True) > 0
    g.add_nodes([f"n{i}" for i in range(1000)], attrs={"name": [f"name{i}" for i in range(1000)]})
    g.add_edges([(f"n{i}", f"n{i + 1}") for i in range(999)])
    structure = g.memory_usage()
    assert structure > empty + 1000 * 40
    deep = g.memory_usage(deep=True)
    # 1000 attribute dicts and names on top
    assert deep > structure + 1000 * 100
    g.create_index("name")
    assert g.memory_usage() > structure


@pytest.mark.parametrize("fmt", ["json", "bin", "json_str"])
def test_indexes_are_saved_and_rebuilt(tmp_path, fmt):
    g = iw.Vertex()
    for i in range(20):
        g.add_node(f"n{i}", {"age": i % 5, "city": ["Oslo", "Rome"][i % 2]})
    g.create_index("age")
    g.create_index("city")
    if fmt == "json":
        g.save_to_json(str(tmp_path / "g.json"))
        h = iw.Vertex.load_from_json(str(tmp_path / "g.json"))
    elif fmt == "json_str":
        h = iw.Vertex.load_from_json(g.save_to_json())
    else:
        g.save_to_binary(str(tmp_path / "g.bin"))
        h = iw.Vertex.load_from_binary(str(tmp_path / "g.bin"))
    assert h.indexes == ["age", "city"]
    assert [n.id for n in h.find("age", 3)] == ["n3", "n8", "n13", "n18"]
    assert len(h.find("city", "Rome")) == 10
    # Still follows changes after loading
    h["n0"].attr_set("age", 3)
    assert [n.id for n in h.find("age", 3)] == ["n0", "n3", "n8", "n13", "n18"]


def test_index_stats():
    g = people()
    assert g.index_stats("city") is None
    g.create_index("city")
    g.create_index("age")
    stats = g.index_stats("city")
    assert stats == {"entries": 4, "distinct_keys": 2, "memory_bytes": stats["memory_bytes"], "dirty": 0}
    assert 0 < stats["memory_bytes"] <= g.memory_usage()
    # 25.0 and 31 / 40 are numbers, "unknown" a string: four distinct keys
    assert g.index_stats("age")["entries"] == 4
    assert g.index_stats("age")["distinct_keys"] == 4
    g["bob"].attr_set("city", "Paris")
    g.add_node("eve", {"city": "Oslo"})
    g.remove_node("acme")
    stats = g.index_stats("city")
    assert (stats["entries"], stats["distinct_keys"], stats["dirty"]) == (4, 2, 0)
    g.drop_index("city")
    assert g.index_stats("city") is None
