"""Pattern matching (Vertex.match) and variable-length paths (Node.paths)."""

import random

import pytest

from ironweaver import Edge, Node, Path, Vertex, attr, edge_type, label


def company():
    g = Vertex()
    for id, age in [("ann", 31), ("bob", 25), ("cat", 40), ("dan", 19)]:
        g.add_node(id, {"age": age}, labels=["Person"])
    g.add_node("acme", {"name": "Acme"}, labels=["Company"])
    g.add_node("init", {"name": "Init"}, labels=["Company"])
    for a, b, t in [
        ("ann", "bob", "knows"),
        ("bob", "cat", "knows"),
        ("cat", "ann", "knows"),
        ("bob", "dan", "knows"),
        ("ann", "acme", "works_at"),
        ("bob", "acme", "works_at"),
        ("cat", "init", "works_at"),
    ]:
        g.add_edge(a, b, {"since": 2000 + len(a + b)}, type=t)
    return g


def rows(matches, *names):
    return sorted(tuple(m[n].id if isinstance(m[n], Node) else len(m[n]) for n in names) for m in matches)


def test_match_basics():
    g = company()
    got = g.match("(p:Person)-[w:works_at]->(c:Company {name: 'Acme'})")
    assert rows(got, "p", "c") == [("ann", "acme"), ("bob", "acme")]
    assert set(got[0]) == {"p", "w", "c"}  # only named variables
    assert isinstance(got[0]["w"], Edge) and got[0]["w"].type == "works_at"
    assert got[0]["w"].from_node == got[0]["p"]
    # Colleagues: different edges, nodes may repeat
    got = g.match("(a)-[:works_at]->(c)<-[:works_at]-(b)")
    assert rows(got, "a", "b") == [("ann", "bob"), ("bob", "ann")]
    # Either direction, alternatives, cycles
    assert rows(g.match("(a {age: 19})-[:knows|works_at]-(b)"), "b") == [("bob",)]
    cyc = g.match("(a)-[:knows]->(b)-[:knows]->(c)-[:knows]->(a)")
    assert rows(cyc, "a", "b", "c") == [("ann", "bob", "cat"), ("bob", "cat", "ann"), ("cat", "ann", "bob")]
    # Several paths share variables
    got = g.match("(a:Person)-[:works_at]->(c), (a)-[:knows]->(b)")
    assert rows(got, "a", "b", "c") == [("ann", "bob", "acme"), ("bob", "cat", "acme"), ("bob", "dan", "acme"), ("cat", "ann", "init")]


def test_match_where_ids_limit():
    g = company()
    got = g.match("(a)-[k:knows]->(b)", where={"b": attr("age") < 30, "k": attr("since") > 2005})
    assert rows(got, "a", "b") == [("ann", "bob"), ("bob", "dan")]
    got = g.match("(a)-[:knows]->(b)", where={"a": label("Person") & (attr("age") > 30)})
    assert rows(got, "a", "b") == [("ann", "bob"), ("cat", "ann")]
    assert rows(g.match("(a)-->(b)", ids={"a": "bob"}), "b") == [("acme",), ("cat",), ("dan",)]
    assert rows(g.match("(a)-->(b)", ids={"a": ["cat", "nobody"]}), "b") == [("ann",), ("init",)]
    assert g.match("(a)-->(b)", ids={"a": "nobody"}) == []
    assert len(g.match("(a)-->(b)", limit=3)) == 3
    assert g.match("(a)-[:nope]->(b)") == [] and g.match("(a:Nope)") == []
    assert g.match("(a)-[r]->(b)", where={"r": edge_type("works_at")}, limit=1)[0]["r"].type == "works_at"


def test_match_variable_length():
    g = company()
    got = g.match("(a)-[p:knows*1..2]->(b)", ids={"a": "ann"})
    assert rows(got, "b", "p") == [("bob", 1), ("cat", 2), ("dan", 2)]
    # Zero hops bind the start; edges come in path order
    got = g.match("(a)-[p:knows*0..3]->(b:Person)", ids={"a": "ann"})
    assert rows(got, "b", "p") == [("ann", 0), ("ann", 3), ("bob", 1), ("cat", 2), ("dan", 2)]
    loop = next(m["p"] for m in got if len(m["p"]) == 3)
    assert [e.from_node.id for e in loop] == ["ann", "bob", "cat"]
    # Matching may start from the far end: edges still go from a to b
    got = g.match("(a)-[p:knows*2]->(b)", ids={"b": "dan"})
    assert [[e.from_node.id for e in m["p"]] for m in got] == [["ann", "bob"]]
    # Edges bound elsewhere are not reused
    got = g.match("(a)-[:knows]->(b)-[p:knows*]->(c)", ids={"a": "ann"})
    assert rows(got, "c", "p") == [("ann", 2), ("cat", 1), ("dan", 1)]


def test_match_errors():
    g = company()
    with pytest.raises(ValueError, match="expected '\\)'"):
        g.match("(a")
    with pytest.raises(ValueError, match="no variable 'zz'"):
        g.match("(a)", where={"zz": attr("x") == 1})
    with pytest.raises(ValueError, match="no variable 'r'"):
        g.match("(a)-[r]->(b)", ids={"r": "ann"})
    with pytest.raises(TypeError, match="Expr"):
        g.match("(a)", where={"a": {"age": 3}})
    with pytest.raises(ValueError, match="less than min_hops"):
        g.match("(a)-[*3..2]->(b)")


def random_simple_digraph(seed, n=12, m=30):
    nx = pytest.importorskip("networkx")
    rng = random.Random(seed)
    g, h = Vertex(), nx.DiGraph()
    for i in range(n):
        g.add_node(f"n{i}", {"w": rng.randrange(3)})
        h.add_node(f"n{i}")
    while h.number_of_edges() < m:
        a, b = f"n{rng.randrange(n)}", f"n{rng.randrange(n)}"
        if a != b and not h.has_edge(a, b):
            g.add_edge(a, b, type=rng.choice(["x", "y"]))
            h.add_edge(a, b)
    return g, h


@pytest.mark.parametrize("seed", range(5))
def test_against_networkx(seed):
    nx = pytest.importorskip("networkx")
    g, h = random_simple_digraph(seed)
    # Directed triangles: each found once per rotation
    cycles = [c for c in nx.simple_cycles(h, length_bound=3) if len(c) == 3]
    assert len(g.match("(a)-->(b)-->(c)-->(a)")) == 3 * len(cycles)
    # Simple paths from a node, up to 4 edges
    want = sorted(tuple(p) for t in h if t != "n0" for p in nx.all_simple_paths(h, "n0", t, cutoff=4))
    got = sorted(tuple(p.ids()) for p in g["n0"].paths(1, 4, uniqueness="path"))
    assert got == want
    # The same through a pattern: every (end node, length) pair
    matches = g.match("(a)-[p*1..4]->(b)", ids={"a": "n0"})
    trails = {(m["b"].id, len(m["p"])) for m in matches}
    assert {(p[-1], len(p) - 1) for p in want} <= trails


def test_node_paths():
    g = company()
    ann = g["ann"]
    paths = ann.paths(1, 2)
    assert all(isinstance(p, Path) for p in paths)
    assert [p.ids() for p in paths] == [["ann", "bob"], ["ann", "bob", "cat"], ["ann", "bob", "dan"], ["ann", "bob", "acme"], ["ann", "acme"]]
    assert [len(p) for p in paths] == [1, 2, 2, 2, 1]
    assert paths[1].edges[1].to_node.id == "cat" and paths[1].nodes[0] == ann
    assert [p.ids() for p in ann.paths(0, 0)] == [["ann"]]
    assert [p.ids() for p in ann.paths(2, 2, types="knows")] == [["ann", "bob", "cat"], ["ann", "bob", "dan"]]
    assert [p.ids() for p in ann.paths(1, 1, direction="in")] == [["ann", "cat"]]
    assert len(ann.paths(1, None, direction="both", types=["knows"], uniqueness="path")) > 0
    got = ann.paths(1, 3, where=attr("since") < 2007)  # the knows edges
    assert [p.ids() for p in got] == [["ann", "bob"], ["ann", "bob", "cat"], ["ann", "bob", "cat", "ann"], ["ann", "bob", "dan"]]
    # Trails may come back round; walks may repeat edges
    assert ["ann", "bob", "cat", "ann"] in [p.ids() for p in ann.paths(3, 3, types="knows")]
    assert ["ann", "bob", "cat", "ann", "bob"] in [p.ids() for p in ann.paths(4, 4, types="knows", uniqueness="walk")]
    assert len(ann.paths(1, None, limit=2)) == 2
    with pytest.raises(ValueError, match="max_hops"):
        ann.paths(1, None, uniqueness="walk")
    with pytest.raises(ValueError, match="uniqueness"):
        ann.paths(uniqueness="nope")
    with pytest.raises(TypeError, match="Expr"):
        ann.paths(where={"since": 1})
    assert repr(paths[0]) == 'Path(["ann", "bob"])'
