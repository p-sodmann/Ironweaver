"""Attribute updates must release graph borrows before running Python code."""

import pytest

from ironweaver import Vertex


@pytest.fixture(params=["node", "edge"])
def target(request):
    graph = Vertex()
    node = graph.add_node("a")
    graph.add_node("b")
    if request.param == "node":
        return graph, node, "on_node_update_callbacks"
    return graph, graph.add_edge("a", "b"), "on_edge_update_callbacks"


def test_missing_none_and_equal_values(target):
    graph, item, callback_name = target
    events = []
    getattr(graph, callback_name).append(lambda *args: events.append(args))
    item.attr_set("x", None)
    item.attr_set("x", None)
    item.attr_set("x", 1)
    item.attr_set("x", 1.0)
    assert events == [(graph, item, "x", None, None), (graph, item, "x", 1, None)]
    assert type(item.attr_get("x")) is float  # equal values still replace the value


def test_equality_may_derive_and_mutate_graph_and_replace_callbacks(target):
    graph, item, callback_name = target
    derived = []
    events = []

    class Old:
        def __eq__(self, other):
            derived.append(graph.filter(ids=["a", "b"]))
            graph.add_node("during_equality")
            setattr(graph, callback_name, [lambda *args: events.append(args)])
            return False

    old = Old()
    item.attr_set("x", old)
    getattr(graph, callback_name).append(lambda *args: pytest.fail("stale callback list"))
    item.attr_set("x", "new")
    assert graph.has_node("during_equality")
    copy = (
        derived[0]["a"]
        if callback_name == "on_node_update_callbacks"
        else derived[0]["a"].edges[0]
    )
    assert copy.attr_get("x") is old
    assert item.attr_get("x") == "new"
    assert len(events) == 1
    vertex, handle, key, new, previous = events[0]
    assert vertex is graph and handle == item
    assert (key, new) == ("x", "new") and previous is old


@pytest.mark.parametrize("failure", ["compare", "truth"])
def test_equality_errors_abort_update(target, failure):
    graph, item, callback_name = target
    events = []

    class Truth:
        def __bool__(self):
            raise LookupError("truth failed")

    class Old:
        def __eq__(self, other):
            if failure == "compare":
                raise LookupError("compare failed")
            return Truth()

    old = Old()
    item.attr_set("x", old)
    getattr(graph, callback_name).append(lambda *args: events.append(args))
    with pytest.raises(LookupError, match=f"{failure} failed"):
        item.attr_set("x", "new")
    assert item.attr_get("x") is old and events == []


def test_callbacks_mutate_graph_and_false_stops_dispatch(target):
    graph, item, callback_name = target
    events = []

    def first(vertex, handle, key, new, old):
        assert vertex is graph and handle == item and (key, new, old) == ("x", 2, None)
        assert handle.attr_get(key) == new
        vertex.add_node("callback_node")
        events.append("first")
        return False

    getattr(graph, callback_name).extend([first, lambda *args: events.append("second")])
    item.attr_set("x", 2)
    assert graph.has_node("callback_node") and events == ["first"]


def test_callback_errors_propagate_after_mutation(target):
    graph, item, callback_name = target
    events = []

    def fail(*args):
        raise LookupError("callback failed")

    getattr(graph, callback_name).extend([fail, lambda *args: events.append(args)])
    with pytest.raises(LookupError, match="callback failed"):
        item.attr_set("x", 2)
    assert item.attr_get("x") == 2 and events == []


def test_reserved_updates_and_validation(target):
    graph, item, callback_name = target
    if callback_name == "on_node_update_callbacks":
        key, first, second, invalid = "labels", ["Person"], ["Person", "Active"], [1]
    else:
        key, first, second, invalid = "type", "KNOWS", None, ["bad"]
    events = []
    getattr(graph, callback_name).append(lambda *args: events.append(args))
    old = item.attr_get(key)
    item.attr_set(key, first)
    item.attr_set(key, first)
    item.attr_set(key, second)
    with pytest.raises(TypeError, match=f'"{key}"'):
        item.attr_set(key, invalid)
    assert events == [(graph, item, key, first, old), (graph, item, key, second, first)]
    assert item.attr_get(key) == second
    if key == "labels":
        assert [node.id for node in graph.nodes_with_label("Active")] == ["a"]
    else:
        assert item.type is None


def test_node_index_is_current_during_callback():
    graph = Vertex()
    node = graph.add_node("a", {"score": 1})
    graph.create_index("score")
    seen = []
    graph.on_node_update_callbacks.append(
        lambda vertex, *args: seen.append([n.id for n in vertex.find("score", 2)])
    )
    node.attr_set("score", 2)
    assert seen == [["a"]]
