# Analytics

Graph algorithms run on a [`Projection`](traversal.md#projections--vertexproject): a compact, read-only copy of (part of) the graph made by `vertex.project(...)`. Build it once and run as many algorithms on it as you like. They run in Rust with the GIL released, several of them on all cores. Results use node ids: dicts `{id: value}` for per-node scores, lists of lists of ids (largest first) for groups of nodes.

The examples below use this graph:

```python
from ironweaver import Vertex

g = Vertex()
for id in ["a", "b", "c", "d", "e", "f", "x"]:
    g.add_node(id)
for src, dst in [("a", "b"), ("b", "c"), ("c", "a"), ("c", "d"), ("d", "e"), ("e", "d"), ("f", "e")]:
    g.add_edge(src, dst, {"weight": 1.0})
#   a -> b -> c -> a (a cycle),  c -> d <-> e <- f,  x on its own

p = g.project()
```

Directed algorithms follow the projection's edges, so use `direction="in"` to reverse them and `direction="both"` to treat them as undirected. Triangles, clustering, core numbers and label propagation always treat edges as undirected, and ignore self-loops and parallel edges (like networkx on `nx.Graph(G)`).

## Components

```python
assert p.weakly_connected_components() == [["a", "b", "c", "d", "e", "f"], ["x"]]
assert p.strongly_connected_components() == [["a", "b", "c"], ["d", "e"], ["f"], ["x"]]
```

Groups are ordered largest first. Equal sizes keep projection order, and members are in projection order too (the graph's node order).

## Cycles and topological order

```python
assert p.find_cycle() == ["a", "b", "c"]          # the last node has an edge back to the first
try:
    p.topological_sort()
except ValueError as e:
    assert "cycle" in str(e)

dag = g.project(nodes=["c", "d", "f"])            # c -> d only
assert dag.find_cycle() is None
assert dag.topological_sort() == ["c", "d", "f"]  # ties broken by projection order
```

- `topological_sort()` raises `ValueError` if there is a cycle. A self-loop counts as a cycle.
- `find_cycle()` returns one cycle, or `None`.

## Centrality

```python
dc = p.degree_centrality()                        # out-degree / (n - 1); "in" for in-degree
assert dc["c"] == 2 / 6 and p.degree_centrality("in")["e"] == 2 / 6

pr = p.pagerank()                                 # {id: rank}, the ranks sum to 1
assert abs(sum(pr.values()) - 1) < 1e-9
assert pr["x"] == min(pr.values())

ppr = p.pagerank(personalization={"a": 1.0})      # personalized: random jumps go back to "a"
assert ppr["x"] == 0.0 and ppr["b"] > ppr["f"]
```

- `pagerank(alpha=0.85, *, personalization=None, max_iter=100, tol=1e-6)` gives the same results as `networkx.pagerank`.
  - It uses the projection's weights if it has them (`g.project(weight="weight")`), and parallel edges add up.
  - Nodes without outgoing edges jump according to `personalization`, which is uniform when it isn't given.
  - It raises `ValueError` for invalid options or if it doesn't converge within `max_iter`.
- `degree_centrality("out" | "in")` counts one per edge. On a `direction="both"` projection it matches networkx's `degree_centrality` (in + out).

## Local structure

```python
assert p.triangles() == {"a": 1, "b": 1, "c": 1, "d": 0, "e": 0, "f": 0, "x": 0}
cc = p.clustering()
assert cc["a"] == 1.0 and abs(cc["c"] - 1 / 3) < 1e-12
assert p.core_number() == {"a": 2, "b": 2, "c": 2, "d": 1, "e": 1, "f": 1, "x": 0}
```

- `triangles()` gives the number of triangles through each node.
- `clustering()` gives the local clustering coefficient: the fraction of pairs of a node's neighbours that are neighbours themselves.
- `core_number()` gives each node's k-core number: the largest `k` for which the node belongs to a subgraph where every node has at least `k` neighbours.

## Communities

```python
teams = Vertex()
groups = [["ann", "bob", "cat", "dan"], ["eve", "fay", "gus", "hal"]]
for group in groups:
    for id in group:
        teams.add_node(id)
    for i, a in enumerate(group):
        for b in group[i + 1:]:
            teams.add_edge(a, b)                  # everyone in a group knows each other
teams.add_edge("dan", "eve")                      # one link between the groups
communities = teams.project().label_propagation()  # max_iter=20
assert sorted(map(sorted, communities)) == groups
```

This is synchronous label propagation, the LDBC Graphalytics "CDLP" rule, with edges treated as undirected:
- Every node starts with its own label.
- In each round, every node takes the label most common among its neighbours; ties go to the smallest label.
- It stops when no label changes, or after `max_iter` rounds (synchronous updates can oscillate).
- The result is deterministic.

## Breadth-first levels

```python
assert p.bfs_levels(["a"]) == {"a": 0, "b": 1, "c": 2, "d": 3, "e": 4}
assert p.bfs_levels(["a", "f"], max_depth=1) == {"a": 0, "b": 1, "f": 0, "e": 1}
```

`bfs_levels(sources, max_depth=None)` gives the hop distance from the nearest source to every node it reaches. It is a parallel, direction-optimizing BFS: it switches to scanning the unvisited nodes when the frontier is large.
