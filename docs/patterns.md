# Pattern matching and variable-length paths

`Vertex.match` finds every occurrence of a pattern written in a Cypher-like syntax. `Node.paths` lists the paths of a given length range from one node. Both run in Rust, and their conditions are [expressions](filtering.md) (`attr`, `label`, `edge_type`), so no Python function is called per node or edge.

The examples use this graph:

```python
from ironweaver import Vertex, attr, label

g = Vertex()
for id, age in [("ann", 31), ("bob", 25), ("cat", 40), ("dan", 19)]:
    g.add_node(id, {"age": age}, labels=["Person"])
g.add_node("acme", {"name": "Acme"}, labels=["Company"])
g.add_node("init", {"name": "Init"}, labels=["Company"])
for a, b, t in [("ann", "bob", "knows"), ("bob", "cat", "knows"), ("cat", "ann", "knows"),
                ("bob", "dan", "knows"), ("ann", "acme", "works_at"), ("bob", "acme", "works_at"),
                ("cat", "init", "works_at")]:
    g.add_edge(a, b, type=t)
```

## Patterns

```python
found = g.match("(p:Person)-[w:works_at]->(c:Company {name: 'Acme'})")
assert sorted(m["p"].id for m in found) == ["ann", "bob"]
m = found[0]
assert set(m) == {"p", "w", "c"} and m["w"].type == "works_at"   # Node, Edge and Node handles
```

Each match is a dict from variable name to a `Node`, an `Edge`, or, for a variable-length edge, a list of `Edge`s. Anonymous parts (`()`, `-->`) are matched but not reported.

The syntax:
- **Nodes** are written `(name:Label1:Label2 {key: value, ...})`, and every part is optional.
  - A node must carry all the labels listed.
  - `{key: value}` requires the attribute to equal a string (`'...'` or `"..."`), a number, `true` or `false`.
  - Writing the same name twice refers to the same node.
- **Edges** are written `-[name:TYPE1|TYPE2 {key: value}]->` or `<-[...]-`, or `-[...]-` for either direction. Without a body, use `-->`, `<--` or `--`.
  - Types are alternatives; an edge without types matches any edge.
- **Lengths** make an edge variable-length: `*` (1 or more), `*3` (exactly 3), `*1..3`, `*..3` (1 to 3) or `*2..` (2 or more).
- **Several paths** are separated by commas and share variables: `"(a)-->(b), (a)-->(c)"`.
- Names and labels are identifiers, or any text in backticks.

The semantics are Cypher's:
- Every pattern edge matches a different graph edge.
- Nodes may repeat: two variables can match the same node.
- An undirected pattern edge matches each edge from both ends, so `(a)--(b)` finds every connected pair both ways round.

```python
colleagues = g.match("(a)-[:works_at]->(c)<-[:works_at]-(b)")
assert sorted((m["a"].id, m["b"].id) for m in colleagues) == [("ann", "bob"), ("bob", "ann")]

cycles = g.match("(a)-[:knows]->(b)-[:knows]->(c)-[:knows]->(a)")
assert len(cycles) == 3                                        # one per starting point

both = g.match("(a:Person)-[:works_at]->(c), (a)-[:knows]->(b)")  # two paths sharing a
assert len(both) == 4
```

### Conditions, fixed nodes and limits

```python
young = g.match("(a)-[:knows]->(b)", where={"b": attr("age") < 30})
assert sorted((m["a"].id, m["b"].id) for m in young) == [("ann", "bob"), ("bob", "dan")]

from_bob = g.match("(a)-->(b)", ids={"a": "bob"})               # a is bob; a list of ids works too
assert sorted(m["b"].id for m in from_bob) == ["acme", "cat", "dan"]

assert len(g.match("(a)-->(b)", limit=2)) == 2
```

- `where={name: Expr}` adds conditions to node or edge variables. They combine with those written in the pattern.
- `ids={name: id or [ids]}` fixes node variables to given nodes. This is the fastest way to anchor a pattern.
- `limit` stops after that many matches.

Matching starts at the most selective node: one with fixed ids first, then the one with the rarest label, preferring nodes with conditions. It then follows pattern edges from nodes already matched. Give a label, ids or a condition on at least one node of a large graph, or matching starts from every node.

### Variable-length edges

```python
reach = g.match("(a)-[p:knows*1..2]->(b)", ids={"a": "ann"})
assert sorted((m["b"].id, len(m["p"])) for m in reach) == [("bob", 1), ("cat", 2), ("dan", 2)]
path = next(m["p"] for m in reach if m["b"].id == "cat")
assert [e.from_node.id for e in path] == ["ann", "bob"]         # edges in order, from a to b
```

- A variable-length edge matches a trail: no edge twice.
- Its edges can't match any other pattern edge.
- The minimum can be 0, which binds the end to the start with an empty list.
- The edge list always runs from the left node to the right one of the pattern (for `<-[p*]-`, from the right node to the left one).

## Paths from a node

```python
ann = g["ann"]
paths = ann.paths(1, 2)                                         # 1 or 2 edges, following edges forwards
assert [p.ids() for p in paths] == [["ann", "bob"], ["ann", "bob", "cat"], ["ann", "bob", "dan"],
                                    ["ann", "bob", "acme"], ["ann", "acme"]]
p = paths[1]
assert len(p) == 2 and p.edges[1].to_node.id == "cat"           # Path: .nodes, .edges, .ids()

knows = ann.paths(2, 2, types="knows")
assert [p.ids() for p in knows] == [["ann", "bob", "cat"], ["ann", "bob", "dan"]]
simple = ann.paths(1, None, direction="both", uniqueness="path", limit=100)
```

`Node.paths(min_hops=1, max_hops=None, *, direction="out", types=None, where=None, uniqueness="trail", limit=None)` returns `Path` objects in depth-first order (like `match`, a search that explodes can be stopped with Ctrl+C, which raises `KeyboardInterrupt`):
- `direction` is `"out"`, `"in"` or `"both"`.
- `types` gives the edge type(s) to follow; `where` is an `Expr` every edge must match.
- `uniqueness` says what may repeat:
  - `"trail"` (the default): no edge twice, like Cypher.
  - `"path"`: no node twice (simple paths).
  - `"walk"`: anything may repeat, so it needs `max_hops`.
- `limit` stops after that many paths. The number of paths can grow exponentially with the length, so bound `max_hops` or `limit` on dense graphs.
