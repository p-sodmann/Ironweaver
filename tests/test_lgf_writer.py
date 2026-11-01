from ironweaver import Vertex, parse_lgf, to_lgf
import pytest


def _signature(graph: Vertex):
    nodes = {
        node_id: dict(graph.get_node(node_id).attr) for node_id in graph.keys()
    }
    edges = sorted(
        (node_id, edge.to_node.id, repr(sorted(edge.attr.items())))
        for node_id in graph.keys()
        for edge in graph.get_node(node_id).edges
    )
    return nodes, edges


def test_to_lgf_round_trips_through_parse_lgf():
    graph = Vertex()
    graph.add_node(
        "alice",
        {
            "name": 'Alice "Al" [admin]',
            "empty": "",
            "active": True,
            "age": 30,
            "score": 0.5,
            "tags": ["a, b", "c"],
            "zip": "01234",
        },
        labels=["Person", "Admin"],
    )
    graph.add_node("bob", {}, labels=["Person"])
    graph.add_edge("alice", "bob", {"since": 2020, "note": "x = y"}, type="knows")

    assert _signature(parse_lgf(to_lgf(graph))) == _signature(graph)


def test_to_lgf_writes_nodes_in_the_given_order_with_banners():
    graph = Vertex()
    graph.add_node("b", {})
    graph.add_node("a", {})

    text = to_lgf(graph, order=["a"], banners={"a": "First"}, header="Title")

    assert text.startswith("# Title\n")
    assert text.index("# First") < text.index("\na\n") < text.index("\nb\n")


@pytest.mark.parametrize(
    "attrs",
    [{"text": "two\nlines"}, {"items": ["has ] bracket"]}, {"value": {"a": 1}}],
)
def test_to_lgf_rejects_values_lgf_cannot_represent(attrs):
    graph = Vertex()
    graph.add_node("n", attrs)

    with pytest.raises(ValueError):
        to_lgf(graph)


def test_to_lgf_rejects_ids_with_whitespace():
    graph = Vertex()
    graph.add_node("has space", {})

    with pytest.raises(ValueError):
        to_lgf(graph)
