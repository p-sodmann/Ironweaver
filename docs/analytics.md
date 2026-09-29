# Analytics

Graph algorithms run on a [`Projection`](traversal.md#projections--vertexproject): a compact, read-only copy of (part of) the graph made by `vertex.project(...)`. Build it once and run as many algorithms on it as you like. They run in Rust with the GIL released, several of them on all cores. Results use node ids: dicts `{id: value}` for per-node scores, lists of lists of ids (largest first) for groups of nodes.

Ctrl+C stops a long computation: it raises `KeyboardInterrupt` within a fraction of a second and throws the partial result away; the graph and the projection stay usable. (Projections under 20,000 nodes plus edges run without that check: they finish quickly.)

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

Directed algorithms follow the projection's edges, so use `direction="in"` to reverse them and `direction="both"` to treat them as undirected.
- Triangles, clustering, core numbers and similarity treat edges as undirected, and ignore self-loops and parallel edges (like networkx on `nx.Graph(G)`). `clustering(directed=True)` and label propagation follow the LDBC Graphalytics definitions on directed projections.
- Shortest-path based algorithms (betweenness, closeness, harmonic centrality, k shortest paths) ignore self-loops and take the lightest of parallel edges.
- Leiden, modularity and spanning trees treat edges as undirected but keep their weights.

Randomised algorithms (sampled betweenness, Leiden, FastRP, node2vec) take a `seed`: the same seed gives the same result, whatever the number of cores.

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
  - It raises `ValueError` for invalid options or if it doesn't converge within `max_iter`. `tol=0` instead runs exactly `max_iter` iterations, as LDBC Graphalytics does.
- `degree_centrality("out" | "in")` counts one per edge. On a `direction="both"` projection it matches networkx's `degree_centrality` (in + out).

### Betweenness, closeness and harmonic centrality

```python
bc = p.betweenness_centrality(normalized=False)   # shortest paths through each node
assert bc["c"] == 5.0 and bc["f"] == 0.0
approx = p.betweenness_centrality(k=4, seed=1)    # estimated from 4 random sources

close = p.closeness_centrality()                  # from the distances *to* each node
assert close["d"] == max(close.values()) and close["x"] == 0.0
harmonic = p.harmonic_centrality()                # sum of 1 / distance from every other node
assert harmonic["a"] == 1 / 2 + 1 / 1             # from c (1 hop) and b (2 hops)
```

All three give the same results as networkx (`betweenness_centrality`, `closeness_centrality`, `harmonic_centrality`), and run in parallel.
- `betweenness_centrality(k=None, *, normalized=True, endpoints=False, weighted=None, seed=None)`:
  - It runs one search per source, so it costs O(n·m). With `k`, it samples `k` sources and scales the result up; that is the way to go on large graphs.
  - A `direction="both"` projection counts as undirected, which matters for `normalized=False`.
- `closeness_centrality(*, wf_improved=True, weighted=None)`: `(r - 1) / (sum of distances)` over the `r - 1` nodes that reach the node, scaled by `(r - 1) / (n - 1)` when `wf_improved` (for graphs that aren't connected).
- `harmonic_centrality(*, weighted=None)`: handles unreachable nodes without a correction.
- `weighted=None` uses the projection's weights if it has them; `weighted=False` counts hops.

## Local structure

```python
assert p.triangles() == {"a": 1, "b": 1, "c": 1, "d": 0, "e": 0, "f": 0, "x": 0}
cc = p.clustering()
assert cc["a"] == 1.0 and abs(cc["c"] - 1 / 3) < 1e-12
assert p.core_number() == {"a": 2, "b": 2, "c": 2, "d": 1, "e": 1, "f": 1, "x": 0}
```

- `triangles()` gives the number of triangles through each node.
- `clustering()` gives the local clustering coefficient: the fraction of pairs of a node's neighbours that are neighbours themselves.
- `clustering(directed=True)` uses the LDBC Graphalytics definition for directed graphs: the number of directed edges among a node's in- and out-neighbours, divided by `k (k - 1)` for `k` neighbours. On a `direction="both"` projection it is the same as `clustering()`.
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

This is synchronous label propagation, the LDBC Graphalytics "CDLP" rule:
- Every node starts with its own label.
- In each round, every node takes the label most common among its neighbours; ties go to the smallest label (the node earliest in projection order).
- On a directed projection a node's neighbours are its in- and out-neighbours counted separately, so a node linked both ways counts twice. On a `direction="both"` projection each neighbour counts once. Parallel edges and self-loops don't count.
- It stops when no label changes, or after `max_iter` rounds (synchronous updates can oscillate).
- The result is deterministic.

### Leiden

```python
found = teams.project().leiden(seed=1)
assert sorted(map(sorted, found)) == groups
q = teams.project().modularity(found)             # like networkx.community.modularity
assert 0.3 < q < 0.5
```

`leiden(resolution=1.0, *, randomness=0.01, max_iter=3, seed=None)` finds communities by the Leiden algorithm, maximising modularity. It improves on Louvain: every community is guaranteed to be connected, and it usually finds a better partition.
- Edges count as undirected, weighted by the projection's weights; parallel edges add up.
- A higher `resolution` gives more, smaller communities.
- `randomness` controls how randomly the refinement step merges nodes.
- It runs again from its own result until that changes nothing, at most `max_iter` times. More runs gain little: on the benchmark graphs, 10 instead of 3 raise modularity by 0.1–1%, at 2–3× the time.

`modularity(communities, resolution=1.0)` scores a partition: every node must be in exactly one community (lists or sets of ids).

## Similarity

```python
u = g.project(direction="both")
assert u.similarity([("a", "d"), ("d", "f")]) == [1 / 3, 1 / 2]   # Jaccard
assert u.similarity([("a", "d")], "common_neighbors") == [1]      # both link to c
assert u.most_similar("d", 2) == [("f", 0.5), ("a", 1 / 3)]
top = u.most_similar(k=3)                         # {id: [(other, score), ...]} for every node
```

Scores compare the neighbours of two nodes (edges as undirected). They give the same results as networkx's link prediction functions:
- `"jaccard"`: shared / all neighbours of either.
- `"overlap"`: shared / neighbours of the node with fewer.
- `"common_neighbors"`: the number shared.
- `"adamic_adar"`: sum of `1 / log(degree)` over the shared neighbours.
- `"resource_allocation"`: sum of `1 / degree` over the shared neighbours.
- `"preferential_attachment"`: the product of the degrees (only for `similarity`).

`most_similar(ids=None, k=10, *, metric="jaccard", min_score=0.0)`:
- It gives the `k` best matches of each node among the nodes that share a neighbour with it, best first (ties in projection order).
- `ids` limits the nodes asked about; a single id returns just its list.
- It runs in parallel, without scoring every pair.

## Spanning trees

```python
assert g.project().minimum_spanning_tree() == [("a", "b", 1.0), ("a", "c", 1.0), ("c", "d", 1.0), ("d", "e", 1.0), ("e", "f", 1.0)]
```

`minimum_spanning_tree(*, maximum=False)` returns the edges `(id, id, weight)` of a minimum spanning forest: one tree per connected part, edges as undirected, weighted by the projection's weights (1 if unweighted). `maximum=True` gives the heaviest forest.

## k shortest paths

```python
routes = g.project(direction="both").k_shortest_paths("a", "e", 3)
assert [r["nodelist"] for r in routes] == [["a", "c", "d", "e"], ["a", "b", "c", "d", "e"]]
assert [r["cost"] for r in routes] == [3, 4]
```

`k_shortest_paths(source, target, k, method=None)` gives up to `k` paths without repeated nodes, cheapest first, by Yen's algorithm. Each result is `{"nodelist": [...], "cost": c}`, as for `shortest_paths`. `method` is `"dijkstra"` (weights) or `"bfs"` (edge count); by default, dijkstra on a weighted projection.

## Embeddings

```python
u = teams.project(direction="both")
vectors = u.fastrp(64, seed=1)                    # {id: [64 floats]}
assert len(vectors["ann"]) == 64

walks = u.node2vec_walks(walk_length=10, walks_per_node=2, p=1.0, q=0.5, seed=1)
assert len(walks) == 16 and walks[0][0] == "ann"
```

- `fastrp(dimension=128, *, iteration_weights=[0.0, 1.0, 1.0], self_influence=0.0, normalization_strength=0.0, seed=None)` computes FastRP embeddings, the fast random projection that Neo4j GDS uses by default. Nodes with similar neighbourhoods get similar vectors. There is no training:
  - every node starts from a sparse random vector;
  - each iteration replaces it by the normalised sum of its neighbours' vectors (weighted, if the projection is);
  - the result is the sum of the iterations, weighted by `iteration_weights`, plus `self_influence` times the node's own random vector.
  - `normalization_strength` β scales each random vector by `degree^β`; negative values damp hubs.
  - It follows the projection's edges, so use `direction="both"` for undirected graphs.
- `node2vec_walks(walk_length=80, walks_per_node=10, *, p=1.0, q=1.0, sources=None, seed=None)` makes node2vec random walks, to train a word2vec-style model on.
  - Having come from `t`, the walk goes back to `t` with weight `1/p`, to a neighbour of `t` with weight 1, and further away with weight `1/q`, times the edge weight.
  - `q < 1` explores outwards (like DFS), `q > 1` stays local (like BFS); `p = q = 1` is a plain random walk.
  - The result lists the first walk from every source, then the second, and so on.
  - Walks stop early at nodes without outgoing edges.

## Breadth-first levels

```python
assert p.bfs_levels(["a"]) == {"a": 0, "b": 1, "c": 2, "d": 3, "e": 4}
assert p.bfs_levels(["a", "f"], max_depth=1) == {"a": 0, "b": 1, "f": 0, "e": 1}
```

`bfs_levels(sources, max_depth=None)` gives the hop distance from the nearest source to every node it reaches. It is a parallel, direction-optimizing BFS: it switches to scanning the unvisited nodes when the frontier is large.

## Validation and benchmarks

- `tests/test_algorithms.py` compares every algorithm with networkx on random multigraphs.
- `tests/test_graphalytics.py` runs the [LDBC Graphalytics](https://ldbcouncil.org/benchmarks/graphalytics/) validation graphs through BFS, WCC, CDLP, LCC, PageRank and SSSP, and checks the results against the benchmark's reference outputs.
- `benchmarks/compare_libraries.py` runs the common algorithms with ironweaver, networkx, igraph, rustworkx and networkit on medium-sized graphs, checks that the results agree, and writes the timings to `performance_results/library_comparison.md`. The graphs are two SNAP social networks (GitHub developers and Facebook pages, downloaded once) and a generated Kronecker graph.
