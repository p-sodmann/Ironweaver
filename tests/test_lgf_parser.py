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
  -KNOWS-> n2 
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


def test_parse_lgf_new_syntax():
    """Test the new arrow syntax: -relationship-> and <-relationship-"""
    new_syntax_example = """\
leber_größe_syn_1 Synonym
  synonym = "Normal groß"
  -synonym_of-> leber_größe 
  <-has_synonym- leber_größe

leber_größe Person
  name = "Liver size"
"""
    
    g = parse_lgf(new_syntax_example)
    assert isinstance(g, Vertex)
    assert g.node_count() == 2

    syn_node = g.get_node("leber_größe_syn_1")
    liver_node = g.get_node("leber_größe")
    
    # Check forward relationship: synonym_of
    syn_edges = syn_node.edges
    assert len(syn_edges) == 1
    forward_edge = syn_edges[0]
    assert forward_edge.attr["type"] == "synonym_of"
    assert forward_edge.to_node.id == "leber_größe"
    
    # Check inverse relationship: has_synonym
    liver_edges = liver_node.edges
    assert len(liver_edges) == 1
    inverse_edge = liver_edges[0]
    assert inverse_edge.attr["type"] == "has_synonym"
    assert inverse_edge.to_node.id == "leber_größe_syn_1"
    
    # Check that the synonym node has the inverse edge as well
    syn_inverse_edges = syn_node.inverse_edges
    assert len(syn_inverse_edges) == 1
    assert syn_inverse_edges[0].attr["type"] == "has_synonym"
    assert syn_inverse_edges[0].from_node.id == "leber_größe"


def test_parse_lgf_new_syntax_only():
    """Test that only the new syntax works now (old syntax removed)"""
    new_syntax_example = """\
n1 Person
  name = Alice
  -KNOWS-> n2
    since = 2020

n2 Person
  name = Bob
"""
    
    g = parse_lgf(new_syntax_example)
    assert isinstance(g, Vertex)
    assert g.node_count() == 2

    n1 = g.get_node("n1")
    edges = n1.edges
    assert len(edges) == 1
    e = edges[0]
    assert e.attr["type"] == "KNOWS"
    assert e.to_node.id == "n2"
    assert e.attr["since"] == 2020


def test_parse_lgf_followed_by_issue():
    """Test the specific issue with Followed_by being ignored"""
    followed_by_example = """\
untersuchungsbedingungen Field
  default = "Untersuchungsbedingungen: Gut"  
  -Followed_by-> leber_größe 

leber_größe Field
  name = "Liver size"
"""
    
    g = parse_lgf(followed_by_example)
    assert isinstance(g, Vertex)
    assert g.node_count() == 2

    untersuchungs_node = g.get_node("untersuchungsbedingungen")
    leber_node = g.get_node("leber_größe")
    
    # Check that the Followed_by relationship exists
    edges = untersuchungs_node.edges
    assert len(edges) == 1
    edge = edges[0]
    assert edge.attr["type"] == "Followed_by"
    assert edge.to_node.id == "leber_größe"


def test_parse_lgf_old_syntax_not_supported():
    """Test that old syntax is no longer supported"""
    old_syntax_example = """\
n1 Person
  name = Alice
  -> n2 KNOWS
    since = 2020

n2 Person
  name = Bob
"""
    
    g = parse_lgf(old_syntax_example)
    assert isinstance(g, Vertex)
    assert g.node_count() == 2

    n1 = g.get_node("n1")
    # The old syntax should not create edges anymore
    edges = n1.edges
    assert len(edges) == 0
    
    # Instead, it should be treated as a node attribute
    attributes = dict(n1.attr)
    assert "-> n2 KNOWS" in attributes  # The line gets parsed as an attribute


def test_parse_lgf_list_attributes():
    """Test parsing list attributes in various formats"""
    
    # Test multi-line list
    multi_line_example = """\
me Person
    likes_libraries = [
        "a pretty library",
        "a graph library",
    ]
    name = "Test User"
"""
    
    g = parse_lgf(multi_line_example)
    node = g.get_node("me")
    assert node.attr_get("likes_libraries") == ["a pretty library", "a graph library"]
    assert node.attr_get("name") == "Test User"
    
    # Test single-line list
    single_line_example = """\
you Person
    tags = ["tag1", "tag2", "tag3"]
    count = 42
"""
    
    g2 = parse_lgf(single_line_example)
    node2 = g2.get_node("you")
    assert node2.attr_get("tags") == ["tag1", "tag2", "tag3"]
    assert node2.attr_get("count") == 42
    
    # Test empty list
    empty_list_example = """\
empty Person
    items = []
"""
    
    g3 = parse_lgf(empty_list_example)
    node3 = g3.get_node("empty")
    assert node3.attr_get("items") == []
    
    # Test mixed types in list
    mixed_example = """\
mixed Person
    values = [1, 2.5, "hello", true]
"""
    
    g4 = parse_lgf(mixed_example)
    node4 = g4.get_node("mixed")
    assert node4.attr_get("values") == [1, 2.5, "hello", True]


def test_parse_lgf_list_on_edges():
    """Test parsing list attributes on edges"""
    
    edge_list_example = """\
n1 Person
    name = Alice
    -FRIENDS-> n2
        shared_interests = [
            "coding",
            "music",
        ]
        since = 2020

n2 Person
    name = Bob
"""
    
    g = parse_lgf(edge_list_example)
    n1 = g.get_node("n1")
    edges = n1.edges
    assert len(edges) == 1
    e = edges[0]
    assert e.attr["type"] == "FRIENDS"
    assert e.attr["shared_interests"] == ["coding", "music"]
    assert e.attr["since"] == 2020