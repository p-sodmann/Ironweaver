"""Independent graph clones, including nested and shared Python values."""
import pytest

from ironweaver import Vertex


def test_clone_independence():
    g = Vertex(shuffle_seed=42)
    shared = {"items": [1]}
    g.add_node("a", {"nested": shared, "score": 3}, labels=["Item"])
    g.add_node("b")
    removed_id = g.add_edge("a", "b").id
    g.remove_edge("a", "b")
    edge = g.add_edge("a", "b", {"nested": shared}, type="next")
    g["a"].meta = {"nested": shared}
    edge.meta = {"nested": shared}
    g.meta["nested"] = shared
    g.create_index("score")
    g.on_node_add_callbacks.append(lambda *_: pytest.fail("original callback"))

    c = g.clone()
    assert c.keys() == g.keys()
    assert c.shuffle_seed == 42
    assert c["a"].labels == ["Item"]
    ce = c.get_edge(edge.id)
    assert ce.type == "next" and ce.vertex is c
    with pytest.raises(KeyError):
        c.get_edge(removed_id)
    assert c.indexes == ["score"]
    assert [n.id for n in c.find("score", 3)] == ["a"]
    nested = c["a"].attr_get("nested")
    assert nested is ce.attr_get("nested")
    assert nested is c.meta["nested"]
    assert nested is c["a"].meta["nested"] is ce.meta["nested"]
    nested["items"].append(2)
    assert shared == {"items": [1]}
    c["a"].attr_set("score", 9)
    assert g["a"].attr_get("score") == 3
    assert c.find("score", 3) == []
    c.add_node("new")
    c.remove_node("b")
    assert not g.has_node("new") and g.has_node("b")
    assert c.on_node_add_callbacks == []


def test_clone_empty_and_cyclic_metadata():
    g = Vertex()
    g.meta["cycle"] = g.meta
    c = g.clone()
    assert c.node_count() == 0
    assert c.meta is not g.meta
    assert c.meta["cycle"] is c.meta


def test_clone_propagates_copy_error():
    class Uncopyable:
        def __deepcopy__(self, memo):
            raise ValueError("cannot copy")

    g = Vertex()
    g.add_node("a", {"value": Uncopyable()})
    with pytest.raises(ValueError, match="cannot copy"):
        g.clone()
    assert g.has_node("a")


def test_builtin_values_copied_without_python_deepcopy(monkeypatch):
    import copy

    def fail(*args):
        pytest.fail("built-in copy called Python deepcopy")

    monkeypatch.setattr(copy, "deepcopy", fail)
    g = Vertex()
    cyclic = []
    pair = (cyclic,)
    cyclic.append(pair)
    g.meta.update({"cycle": pair, "values": [None, True, 123, 1.5, 2j, "s", b"b"],
                   "tuple": (1, "immutable"), "dict": {("key", 1): [2]}})
    c = g.clone()
    assert c.meta["cycle"][0][0] is c.meta["cycle"]
    assert c.meta["cycle"] is not pair
    assert c.meta["values"] == g.meta["values"]
    assert c.meta["values"] is not g.meta["values"]
    assert c.meta["tuple"] is g.meta["tuple"]
    c.meta["dict"][("key", 1)].append(3)
    assert g.meta["dict"] == {("key", 1): [2]}


def test_custom_hooks_share_native_memo():
    calls = []

    class Custom(list):
        def __deepcopy__(self, memo):
            import copy

            calls.append(self)
            result = Custom()
            memo[id(self)] = result
            result.extend(copy.deepcopy(v, memo) for v in self)
            return result

    g = Vertex()
    items = []
    custom = Custom([items])
    items.append(custom)
    g.meta.update(items=items, custom=custom, again=custom)
    c = g.clone()
    assert len(calls) == 1
    assert type(c.meta["custom"]) is Custom
    assert c.meta["items"][0] is c.meta["custom"] is c.meta["again"]
    assert c.meta["custom"][0] is c.meta["items"]


def test_deeply_nested_values():
    g = Vertex()
    nested = []
    g.meta["nested"] = nested
    for _ in range(120):
        child = []
        nested.append(child)
        nested = child
    c = g.clone()
    original, cloned = g.meta["nested"], c.meta["nested"]
    for _ in range(120):
        assert original is not cloned
        original, cloned = original[0], cloned[0]
