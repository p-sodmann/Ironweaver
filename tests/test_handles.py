"""Node and Edge objects are handles into a Vertex whose data lives in the
Rust core (ironweaver-core). These tests pin down what that means."""

import gc

import pytest

try:
    from ironweaver import Vertex
except Exception as e:  # pragma: no cover - module unavailable
    pytest.skip(f"ironweaver module unavailable: {e}", allow_module_level=True)


def small():
    g = Vertex()
    for n in "abc":
        g.add_node(n, {"name": n})
    g.add_edge("a", "b", {"type": "x"})
    g.add_edge("b", "c", {"type": "y"})
    return g


def test_handles_compare_by_node():
    g = small()
    assert g["a"] == g["a"] and hash(g["a"]) == hash(g["a"])
    assert g["a"] != g["b"]
    assert len({g["a"], g.get_node("a"), g.nodes["a"]}) == 1
    assert g["a"].edges[0] == g["b"].inverse_edges[0]
    assert g["a"].edges[0].to_node == g["b"]
    other = small()
    assert other["a"] != g["a"]


def test_handles_see_changes_made_through_other_handles():
    g = small()
    a1, a2 = g["a"], g["a"]
    a1.attr_set("k", 1)
    assert a2.attr_get("k") == 1
    g.add_edge("a", "c")
    assert [e.to_node.id for e in a2.edges] == ["b", "c"]


def test_removed_node_handles_raise():
    g = small()
    b = g["b"]
    e = g["a"].edges[0]
    removed = g.remove_node("b")
    assert removed.id == "b" and removed.attr == {"name": "b"}
    assert removed.edges == [] and removed.vertex.keys() == ["b"]
    with pytest.raises(RuntimeError):
        b.attr
    with pytest.raises(RuntimeError):
        e.attr
    # A new node reusing the slot is not reachable through the old handle
    g.add_node("d")
    with pytest.raises(RuntimeError):
        b.id


def test_removed_edge_handles_raise():
    g = small()
    e = g["a"].edges[0]
    assert g.remove_edge("a", "b") == 1
    with pytest.raises(RuntimeError):
        e.to_node


def test_rename_node():
    g = small()
    a = g["a"]
    a.id = "z"
    assert sorted(g.keys()) == ["b", "c", "z"]
    assert g["z"] == a and [e.to_node.id for e in g["z"].edges] == ["b"]
    with pytest.raises(ValueError):
        a.id = "b"


def test_filter_may_read_but_not_restructure_the_graph():
    g = small()
    seen = []
    g["a"].bfs(filter=lambda e: seen.append(e.from_node.attr["name"]) or True)
    assert seen == ["a", "b"]
    with pytest.raises(RuntimeError):
        g["a"].bfs(filter=lambda e: g.add_node("new") and True)
    # The graph is still usable afterwards
    g.add_node("new")
    assert g.has_node("new")


def test_callbacks_may_modify_the_graph():
    g = Vertex()

    def on_add(v, node):
        if not node.id.startswith("shadow_"):
            v.add_node("shadow_" + node.id)
            v.add_edge(node.id, "shadow_" + node.id)

    g.on_node_add_callbacks.append(on_add)
    g.add_node("a")
    assert sorted(g.keys()) == ["a", "shadow_a"]
    assert [e.to_node.id for e in g["a"].edges] == ["shadow_a"]

    def on_update(v, node, key, new, old):
        v[node.id].attr = {**node.attr, "seen": True}

    g.on_node_update_callbacks.append(on_update)
    g["a"].attr_set("k", 1)
    assert g["a"].attr == {"k": 1, "seen": True}


def test_traversal_results_are_copies_with_internal_edges():
    g = small()
    r = g["a"].bfs(depth=1)
    assert r.meta["nodelist"] == ["a", "b"]
    assert [e.to_node.id for e in r["a"].edges] == ["b"]
    assert r["b"].edges == []  # b -> c leaves the result
    r["a"].attr_set("name", "changed")
    assert g["a"].attr["name"] == "a"


def test_cycles_through_attributes_are_collected():
    class Tracker:
        finalized = 0

        def __del__(self):
            Tracker.finalized += 1

    for _ in range(3):
        g = small()
        a = g["a"]
        a.attr_set("self", a)        # vertex -> attr -> handle -> vertex
        a.attr_set("tracker", Tracker())
        g.meta["graph"] = g
        del g, a
    gc.collect()
    assert Tracker.finalized == 3


def test_derived_graphs_do_not_see_each_others_attribute_changes():
    # Derived graphs share attribute dicts copy-on-write; changes on either
    # side must stay on that side.
    g = small()
    subs = [g.filter(ids=["a", "b"]), g["a"].bfs(), g["a"].traverse(),
            g.shortest_path("a", "c"), Vertex.from_nodes({"a": g["a"]})]
    g["a"].attr_set("name", "changed")
    g["a"].attr_list_append("tags", 1)
    for sub in subs:
        assert sub["a"].attr == {"name": "a"}
    subs[0]["a"].attr_set("name", "sub")
    subs[0]["a"].edges[0].attr_set("type", "sub")
    assert g["a"].attr == {"name": "changed", "tags": [1]}
    assert g["a"].edges[0].attr == {"type": "x"}
    assert subs[1]["a"].attr == {"name": "a"}

