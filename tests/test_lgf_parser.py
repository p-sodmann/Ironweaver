import os
import sys

ROOT = os.path.dirname(os.path.dirname(__file__))
sys.path.insert(0, ROOT)

try:
    from ironweaver import Vertex
except Exception as e:
    import pytest
    pytest.skip(f"ironweaver module unavailable: {e}", allow_module_level=True)

from ironweaver.lgf_parser import parse_igf3


EXAMPLE = """\
n1 Person
  name = Alice
  age = 30
  -> n2 KNOWS
    since = 2020
#
n2 Person
  name = Bob
"""


def test_parse_igf3():
    g = parse_igf3(EXAMPLE)
    assert isinstance(g, Vertex)
    assert g.node_count() == 2

    n1 = g.get_node("n1")
    assert n1.attr_get("name") == "Alice"
    assert n1.attr_get("age") == 30
    assert n1.attr_get("labels") == ["Person"]

    n2 = g.get_node("n2")
    assert n2.attr_get("name") == "Bob"

    edges = n1.edges
    assert len(edges) == 1
    e = edges[0]
    assert e.attr["type"] == "KNOWS"
    assert e.to_node.id == "n2"
    assert e.attr["since"] == 2020
