import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "python"))

try:
    from ironweaver import Vertex
except Exception as e:  # pragma: no cover - handled via pytest skip
    import pytest

    pytest.skip(f"ironweaver module unavailable: {e}", allow_module_level=True)

from ironweaver.lgf_parser import parse_lgf, parse_lgf_file


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


def test_parse_lgf():
    g = parse_lgf(EXAMPLE)
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


def test_parse_lgf_with_import(tmp_path):
    imported = tmp_path / "other.lgf"
    imported.write_text("n2 Person\n  name = Bob\n")

    base = tmp_path / "base.lgf"
    base.write_text(
        f"import({imported.name})\n"
        "n1 Person\n"
        "  name = Alice\n"
        "  age = 30\n"
        "  -> n2 KNOWS\n"
        "    since = 2020\n"
    )

    g = parse_lgf_file(str(base))
    assert isinstance(g, Vertex)
    assert g.node_count() == 2
    n2 = g.get_node("n2")
    assert n2.attr_get("name") == "Bob"
