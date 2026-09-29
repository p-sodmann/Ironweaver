"""Node labels, edge types and persistent edge ids (graph fields, with
"labels" / "type" kept working as attribute names)."""

import pytest

from ironweaver import Vertex, attr, edge_type, label


def graph():
    g = Vertex()
    g.add_node("ann", {"age": 31}, labels=["Person", "Admin"])
    g.add_node("bob", {"age": 25, "labels": ["Person"]})  # "labels" attr becomes labels
    g.add_node("acme", {"labels": "not a list of str"})  # stays an attribute
    g.add_edge("ann", "bob", {"since": 2020}, type="knows")
    g.add_edge("bob", "ann", {"type": "knows"})  # "type" attr becomes the type
    g.add_edge("ann", "acme", {"type": 5})  # not a str: stays an attribute
    g.add_edge("bob", "acme")
    return g


def test_labels():
    g = graph()
    ann, bob, acme = g["ann"], g["bob"], g["acme"]
    assert ann.labels == ["Person", "Admin"] and bob.labels == ["Person"] and acme.labels == []
    assert acme.attr == {"labels": "not a list of str"}
    # "labels" shows up in attr (and attr_get) and can be set through it
    assert ann.attr == {"age": 31, "labels": ["Person", "Admin"]}
    assert ann.attr_get("labels") == ["Person", "Admin"]
    assert ann.has_label("Admin") and not bob.has_label("Admin") and not bob.has_label("Nope")
    assert [n.id for n in g.nodes_with_label("Person")] == ["ann", "bob"]
    assert ann.add_label("X") and not ann.add_label("X")
    assert ann.remove_label("Admin") and not ann.remove_label("Admin")
    bob.labels = ["B"]
    assert bob.labels == ["B"] and [n.id for n in g.nodes_with_label("Person")] == ["ann"]
    bob.attr_set("labels", ["C", "D"])
    assert bob.labels == ["C", "D"]
    bob.attr = {"age": 26}  # no "labels": cleared
    assert bob.labels == [] and bob.attr == {"age": 26}
    bob.attr = {"age": 26, "labels": ["Z"]}
    assert bob.labels == ["Z"] and bob.attr_get("age") == 26
    with pytest.raises(TypeError):
        bob.attr_set("labels", "Z")


def test_types_and_ids():
    g = graph()
    edges = {(e.from_node.id, e.to_node.id): e for n in g for e in n.edges}
    e1, e2, e3, e4 = edges["ann", "bob"], edges["bob", "ann"], edges["ann", "acme"], edges["bob", "acme"]
    assert [e.id for e in (e1, e2, e3, e4)] == [0, 1, 2, 3]  # ids in creation order
    assert (e1.type, e2.type, e3.type, e4.type) == ("knows", "knows", None, None)
    assert e1.attr == {"since": 2020, "type": "knows"} and e3.attr == {"type": 5}
    assert e1.attr_get("type") == "knows" and e1.toJSON() == {"since": 2020, "type": "knows"}
    assert repr(e1) == "knows: ann --> bob"
    e4.type = "works_at"
    assert g.get_edge(3).type == "works_at"
    e4.attr_set("type", "employs")
    assert e4.type == "employs"
    e4.attr = {"w": 1}  # no "type": cleared
    assert e4.type is None and e4.attr == {"w": 1}
    with pytest.raises(TypeError):
        e4.attr_set("type", 3)
    with pytest.raises(KeyError):
        g.get_edge(99)
    # Ids are never reused
    g.remove_edge("bob", "acme")
    assert g.add_edge("bob", "acme").id == 4


def test_reserved_names_in_filters_and_views():
    g = graph()
    # Traversal dict filters and remove_edge match the type field
    assert sorted(g["ann"].bfs(filter={"type": "knows"}).keys()) == ["ann", "bob"]
    assert sorted(g["ann"].traverse(filter=lambda e: e.type == "knows").keys()) == ["ann", "bob"]
    # Projection filters: dict, Expr
    assert g.project(edge_filter={"type": "knows"}).edge_count() == 2
    assert g.project(edge_filter=edge_type("knows")).edge_count() == 2
    assert g.project(node_filter={"labels": ["Person", "Admin"]}).ids() == ["ann"]
    assert g.project(node_filter=label("Person")).ids() == ["ann", "bob"]
    assert sorted(g.filter(label("Person") & (attr("age") > 30)).keys()) == ["ann"]
    assert g.filter(lambda n: n.has_label("Admin")).keys() == ["ann"]
    # Expressions and pattern property maps see the fields too
    assert g.project(edge_filter=attr("type") == "knows").edge_count() == 2
    assert g.project(edge_filter=attr("type").is_in(["knows", "x"])).edge_count() == 2
    assert sorted(g.filter(attr("labels") == ["Person", "Admin"]).keys()) == ["ann"]
    assert len(g.match("(a)-[{type: 'knows'}]->(b)")) == 2
    assert [m["e"].to_node.id for m in g.match("(a {age: 31})-[e]->(b)", where={"e": ~attr("type").exists()})] == ["acme"]
    assert g.remove_edge("bob", "ann", {"type": "knows"}) == 1
    assert g.remove_edge("ann", "bob", {"type": "other"}) == 0


def test_subgraphs_keep_ids_labels_types():
    g = graph()
    sub = g.filter(ids=["ann", "bob"])
    assert sub["ann"].labels == ["Person", "Admin"]
    assert sorted((e.id, e.type) for n in sub for e in n.edges) == [(0, "knows"), (1, "knows")]
    assert sub.add_edge("ann", "ann").id == 4  # past every id of the source graph
    path = g.shortest_path("ann", "acme")
    assert path["ann"].labels == ["Person", "Admin"]


def test_random_walks_use_the_type_field():
    g = Vertex()
    for n in "abc":
        g.add_node(n)
    g.add_edge("a", "b", type="knows")
    g.add_edge("b", "c", {"kind": "likes"})
    walks = g.random_walks("a", 3, 20, include_edge_types=True, seed=1)
    assert ["a", "knows", "b", "unknown", "c"] in walks
    walks = g.random_walks("a", 3, 20, include_edge_types=True, edge_type_field="kind", seed=1)
    assert ["a", "unknown", "b", "likes", "c"] in walks


def test_to_networkx_exports_labels_and_types():
    pytest.importorskip("networkx")
    h = graph().to_networkx()
    assert h.nodes["ann"]["labels"] == ["Person", "Admin"]
    assert h.edges["ann", "bob"]["type"] == "knows"
    assert "labels" not in h.nodes["acme"] or h.nodes["acme"]["labels"] == "not a list of str"
