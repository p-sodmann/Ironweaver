# ironweaver

A fast property graph library for Python, with its engine in Rust. Build and change graphs from Python; run traversals, shortest paths, graph analytics and pattern matching in Rust, most of them on all cores.

```text
pip install ironweaver
```

```python
from ironweaver import Vertex, attr

g = Vertex()
people = [("ann", {"age": 31}), ("bob", {"age": 25}), ("cat", {"age": 40})]
g.add_nodes(people, labels=["Person"])
g.add_edges([("ann", "bob"), ("bob", "cat"), ("cat", "ann")], type="knows")

path = g.shortest_path("ann", "cat", method="bfs")
assert path.meta["nodelist"] == ["ann", "bob", "cat"]

p = g.project()  # read-only snapshot for analytics
assert p.strongly_connected_components() == [["ann", "bob", "cat"]]
ranks = p.pagerank()

matches = g.match("(a:Person)-[:knows]->(b)", where={"b": attr("age") > 30})
assert sorted(m["a"].id for m in matches) == ["bob", "cat"]  # they know someone over 30
```

## Where to go next

- [Graphs, nodes and edges](basics.md): building graphs, labels, edge types, bulk loading, saving.
- [Filtering](filtering.md) and [traversal and paths](traversal.md).
- [Analytics](analytics.md): components, centrality, communities, similarity, embeddings.
- [Pattern matching](patterns.md): Cypher-like patterns and variable-length paths.
- [API reference](api.md): every class and method.
- [Benchmarks](benchmarks.md): speed and agreement with networkx, igraph, rustworkx and networkit.

The engine is also a Rust crate, [`ironweaver-core`](https://crates.io/crates/ironweaver-core) ([API docs](https://docs.rs/ironweaver-core)).
