"""Filter expressions evaluated in Rust: attr(...) comparisons, is_in,
exists, & | ~, in Vertex.filter and Vertex.project."""

import random

import pytest

from ironweaver import Expr, Vertex, attr, edge_type, label


def people(seed=0, n=200):
    rng = random.Random(seed)
    g = Vertex()
    for i in range(n):
        a = {"age": rng.choice([rng.randrange(0, 90), rng.uniform(0, 90), None])}
        if rng.random() < 0.7:
            a["name"] = rng.choice(["ann", "bob", "cat", "dan"])
        if rng.random() < 0.5:
            a["pos"] = {"lat": rng.uniform(-90, 90)}
        if rng.random() < 0.3:
            a["tags"] = rng.sample(["x", "y", "z"], 2)
        g.add_node(f"n{i}", a)
    for _ in range(3 * n):
        g.add_edge(f"n{rng.randrange(n)}", f"n{rng.randrange(n)}", {"w": rng.randrange(10)})
    return g


def ids(v):
    return sorted(v.keys())


def get(n, *path):
    v = n.attr
    for k in path:
        if not isinstance(v, dict) or v.get(k) is None:
            return None
        v = v[k]
    return v


def num(v):
    return isinstance(v, (int, float)) and not isinstance(v, bool)


CASES = [
    (attr("age") > 40, lambda n: num(get(n, "age")) and get(n, "age") > 40),
    (attr("age") <= 20.5, lambda n: num(get(n, "age")) and get(n, "age") <= 20.5),
    (attr("age") == 30, lambda n: num(get(n, "age")) and get(n, "age") == 30),
    (attr("name") != "ann", lambda n: get(n, "name") is not None and get(n, "name") != "ann"),
    (attr("name") < "c", lambda n: isinstance(get(n, "name"), str) and get(n, "name") < "c"),
    (attr("name").is_in(["bob", "dan"]), lambda n: get(n, "name") in ("bob", "dan")),
    (attr("pos.lat") > 0, lambda n: num(get(n, "pos", "lat")) and get(n, "pos", "lat") > 0),
    (attr(["pos", "lat"]) < 0, lambda n: num(get(n, "pos", "lat")) and get(n, "pos", "lat") < 0),
    (attr("tags") == ["x", "y"], lambda n: get(n, "tags") == ["x", "y"]),
    (attr("age").exists(), lambda n: get(n, "age") is not None),
    (~attr("name").exists(), lambda n: get(n, "name") is None),
    ((attr("age") > 18) & (attr("name") == "bob"), lambda n: num(get(n, "age")) and get(n, "age") > 18 and get(n, "name") == "bob"),
    ((attr("age") < 10) | (attr("name") == "cat"), lambda n: (num(get(n, "age")) and get(n, "age") < 10) or get(n, "name") == "cat"),
    (~(attr("age") > 40), lambda n: not (num(get(n, "age")) and get(n, "age") > 40)),
    (attr("name") > 5, lambda n: False),  # incomparable types
]


@pytest.mark.parametrize("seed", range(3))
@pytest.mark.parametrize("case", range(len(CASES)))
def test_filter_matches_python(seed, case):
    g = people(seed)
    expr, fn = CASES[case]
    want = sorted(n.id for n in g if fn(n))
    assert ids(g.filter(expr)) == want
    assert ids(g.filter(where=expr)) == want
    assert g.project(node_filter=expr).ids() == [id for id in g.keys() if id in set(want)]


def test_edge_filters():
    g = people(1)
    p = g.project(edge_filter=attr("w") >= 5)
    want = sum(1 for n in g for e in n.edges if e.attr["w"] >= 5)
    assert p.edge_count() == want
    both = g.project(edge_filter=(attr("w") >= 5) & (attr("w") < 7), node_filter=attr("age").exists())
    assert both.edge_count() <= want
    # Labels / types don't apply to the other kind
    assert g.project(node_filter=edge_type("knows")).node_count() == 0
    assert g.project(edge_filter=label("Person")).edge_count() == 0


def test_repr_and_errors():
    e = (attr("age") >= 18) & ~label("X") | (attr("name") == "q")
    assert isinstance(e, Expr)
    assert repr(e) == '(((attr("age") >= 18) & ~label("X")) | (attr("name") == "q"))'
    assert repr(attr("a").is_in([1, "b", None])) == 'attr("a").is_in([1, "b", None])'
    assert repr(~~attr("a").exists()) == 'attr("a").exists()'
    with pytest.raises(TypeError, match="parentheses"):
        attr("age") > 30 & label("Person")  # noqa: B015 - & binds tighter than >
    with pytest.raises(TypeError, match="combine expressions"):
        bool(attr("a") > 1 and attr("b") < 2)
    with pytest.raises(TypeError, match="combine expressions"):
        1 < attr("x") < 3  # noqa: B015
    with pytest.raises(TypeError, match="compare attr"):
        bool(attr("x"))
    with pytest.raises(ValueError, match="empty"):
        attr("a..b")
    with pytest.raises(TypeError):
        attr(5)
    with pytest.raises(TypeError, match="where= takes an Expr"):
        people().filter(where=5)
    with pytest.raises(TypeError, match="Expr or a callable"):
        people().project(node_filter=5)
    x = attr("a") == 1
    for _ in range(40):
        x = ~(x | (attr("b") == 2))
    with pytest.raises(ValueError, match="nested more than"):
        for _ in range(100):
            x = ~(x | (attr("b") == 2))
    # Long chains flatten instead of nesting
    chain = attr("a") == 0
    for i in range(1000):
        chain = chain | (attr("a") == i)
    assert people().filter(chain).node_count() == 0
