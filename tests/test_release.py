"""Release-facing behaviour: version, deprecations."""

import re
import warnings

import pytest

import ironweaver
from ironweaver import Vertex


def test_version():
    assert re.fullmatch(r"\d+\.\d+\.\d+([.-].+)?", ironweaver.__version__)


def graph():
    g = Vertex()
    g.add_nodes(["a", "b"])
    g.add_edge("a", "b", {"weight": 2.0})
    return g


@pytest.mark.parametrize("name, method", [("shortest_path_bfs", "bfs"), ("shortest_path_dijkstra", "dijkstra")])
def test_legacy_shorthands_are_deprecated(name, method):
    g = graph()
    with pytest.warns(DeprecationWarning, match=rf'{name}\(\) is deprecated.*method="{method}"'):
        old = getattr(g, name)("a", "b")
    new = g.shortest_path("a", "b", method=method)
    assert old.meta["nodelist"] == new.meta["nodelist"] == ["a", "b"]
    # The replacement doesn't warn
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        g.shortest_path("a", "b", method=method)
