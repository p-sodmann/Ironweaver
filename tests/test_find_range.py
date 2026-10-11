"""Range boundary and validation rules agree with and without an index."""

import pytest

from ironweaver import Vertex


@pytest.fixture(params=[False, True], ids=["scan", "indexed"])
def graph(request):
    g = Vertex()
    # Deliberately not in key order: results retain node insertion order.
    for name, value in [("two", 2), ("zero", 0), ("three", 3.0), ("one", 1.0)]:
        g.add_node(name, {"score": value})
    g.add_node("missing")
    g.add_node("none", {"score": None})
    g.add_node("string", {"score": "1"})
    g.add_node("bool", {"score": True})
    if request.param:
        g.create_index("score")
    return g


@pytest.mark.parametrize("inclusive", ["both", "left", "right", "neither"])
@pytest.mark.parametrize(
    "low,high,expected",
    [
        (1, 2.0, {"both": ["two", "one"], "left": ["one"], "right": ["two"], "neither": []}),
        (1.0, 1, {"both": ["one"], "left": [], "right": [], "neither": []}),
        (2, 1, {"both": [], "left": [], "right": [], "neither": []}),
        (
            1,
            None,
            {
                "both": ["two", "three", "one"],
                "left": ["two", "three", "one"],
                "right": ["two", "three"],
                "neither": ["two", "three"],
            },
        ),
        (
            None,
            2,
            {
                "both": ["two", "zero", "one"],
                "left": ["zero", "one"],
                "right": ["two", "zero", "one"],
                "neither": ["zero", "one"],
            },
        ),
    ],
)
def test_range_boundaries(graph, low, high, expected, inclusive):
    found = graph.find_range("score", low, high, inclusive=inclusive)
    assert [node.id for node in found] == expected[inclusive]


SCALAR_ERROR = "range bounds must be numbers, strings, bools, bytes, dates or datetimes"


@pytest.mark.parametrize(
    "low,high,inclusive,message",
    [
        (None, None, "both", "find_range needs low or high"),
        ([], 3, "both", SCALAR_ERROR),
        (0, {}, "both", SCALAR_ERROR),
        (float("nan"), 3, "both", SCALAR_ERROR),
        (0, float("nan"), "both", SCALAR_ERROR),
        (1, "z", "both", "low and high must be of the same kind"),
        ([], None, "all", 'inclusive must be "both", "left", "right" or "neither", got "all"'),
    ],
)
def test_range_validation(graph, low, high, inclusive, message):
    with pytest.raises(ValueError) as exc:
        graph.find_range("score", low, high, inclusive=inclusive)
    assert str(exc.value) == message
    assert [node.id for node in graph.find_range("score", 1, 2)] == ["two", "one"]
