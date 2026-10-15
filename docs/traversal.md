# Traversal

Ironweaver provides several traversal methods on both `Node` and `Vertex`.

## Node-level traversal

All node-level methods start from a single node and follow outgoing edges. They return a new `Vertex` with copies of the discovered nodes (and the edges between them) and a `meta["nodelist"]` recording visit order. A callable `filter` may read the graph but must not change it (adding or removing nodes or edges, setting attributes): that can raise `RuntimeError`.

The examples below use this graph:

```python
from ironweaver import Vertex

v = Vertex()
for name in ["root", "a", "b", "target", "z"]:
    v.add_node(name)
v.add_edge("root", "a", {"type": "knows", "weight": 0.9})
v.add_edge("a", "b", {"type": "follows", "weight": 0.4})
v.add_edge("b", "target", {"type": "knows", "weight": 0.8})
v.add_edge("a", "z", {"type": "knows", "weight": 0.7})
```

### DFS — `node.traverse(depth, filter, edge_filter)`

```python
node = v.get_node("root")
result = node.traverse(depth=3)
result.meta["nodelist"]  # visit order
```

### BFS — `node.bfs(depth, filter, edge_filter)`

```python
result = node.bfs(depth=2)
```

### BFS search — `node.bfs_search(target_id, depth, filter, edge_filter)`

Returns the target `Node` if reachable, otherwise `None`. Stops as soon as the target is found.

```python
found = node.bfs_search("target", depth=5)
```

### Edge filtering

All three methods accept a `filter` parameter to restrict which edges are followed. The filter can be a **dict** for simple attribute matching or a **callable** (lambda) for more expressive logic.

#### Dict filter

Only edges whose `attr` matches **every** key/value pair in the dict are traversed:

```python
result = node.bfs(depth=2, filter={"type": "knows"})
```

#### Lambda filter

Pass a callable that receives an `EdgeView` and returns `True` for edges that should be followed:

```python
result = node.traverse(depth=3, filter=lambda e: e.type == "knows")
result = node.bfs(depth=3, filter=lambda e: e.type in ("knows", "follows"))
found  = node.bfs_search("target", depth=5, filter=lambda e: e.attr("weight") > 0.5)
```

You can also use the explicit `edge_filter` keyword argument (useful when combining with a dict filter):

```python
result = node.bfs(depth=3, edge_filter=lambda e: e.has_attr("type") and e.type == "knows")
```

> **Note:** `filter` accepts either a dict or a callable, but not both. Use `edge_filter` if you need a callable alongside a dict filter.

#### EdgeView API

The `EdgeView` passed to your predicate exposes:

| Property / Method | Description |
|---|---|
| `e.type` | Shortcut for `attr["type"]` |
| `e.attr("key")` | Attribute value, or `None` if missing |
| `e.attr("key", default)` | Attribute value with fallback |
| `e.has_attr("key")` | `True` if the attribute exists |
| `e.attrs` | Full attribute dict |
| `e.from_node` | Source node |
| `e.to_node` | Target node |
| `e.id` | Edge ID (if set) |
| `e.edge` | The underlying `Edge` object |

---

## Vertex-level traversal

### Shortest paths — `vertex.shortest_path(source, target, method=None, ...)`

One method, several algorithms. `method` picks one; `Vertex.path_methods()` lists them with a description:

| `method` | Finds | Method-specific options |
|---|---|---|
| `"bfs"` | fewest edges (bidirectional BFS) | `max_depth` |
| `"dijkstra"` | cheapest by edge weight | – |
| `"astar"` | cheapest by edge weight, guided by a heuristic | `heuristic`, `coords`, `distances` |

With `method=None` it picks `"astar"` if `heuristic`, `coords` or `distances` is given, `"dijkstra"` if `weight` is given, and `"bfs"` otherwise. Shared options: `weight` (edge attribute, default `"weight"`), `default_weight` (cost of edges without it, default 1.0), `max_cost`, and `direction` (`"out"`, `"in"`, `"both"`). An option a method doesn't accept raises `TypeError`.

The result is a new `Vertex` with copies of the path's nodes and the edges between them. `meta["nodelist"]` has the path in order, `meta["cost"]` its cost (the number of edges for `"bfs"`), `meta["method"]` the algorithm used, and `meta["expanded"]` how many nodes Dijkstra/A* settled. `ValueError` if a node is missing or the target is unreachable. If several shortest paths exist, the choice is fixed (see [Equal-cost paths](#equal-cost-paths)).

```python
path = v.shortest_path("root", "target")                     # bfs: fewest edges
path.meta["nodelist"]                                        # ['root', 'a', 'b', 'target']
path = v.shortest_path("root", "target", weight="weight")    # dijkstra: cheapest
path.meta["cost"]                                            # 2.1
path = v.shortest_path("target", "root", method="bfs", direction="in", max_depth=10)
```

The older `shortest_path_bfs(...)` and `shortest_path_dijkstra(...)` still work but are deprecated (they emit a `DeprecationWarning`): use `method="bfs"` and `method="dijkstra"`.

With `method="bfs"`, supplying `weight` or `default_weight` makes infinite
weights impassable, just as on a weighted projection. Finite weights still
count as one hop; missing attributes use `default_weight` (1.0). Negative,
NaN and nonnumeric weights raise errors when examined. Without either option,
BFS ignores weights. `Node.bfs_search` stays unweighted; use an edge filter
to exclude weighted edges there. It only tests reachability and stops as soon
as its frontiers meet.

#### Search statistics

Pass `stats=True` to `Vertex.shortest_path`, `Vertex.shortest_paths` or
`Projection.shortest_paths`. A single path stores `meta["stats"]`; each found
batch path includes a `"stats"` entry. Both contain `{"settled": n}`. Default
results keep their existing shape and keys. Unreachable batch entries remain
`None`, and a single unreachable query still raises `ValueError`.

Dijkstra counts nodes settled (including the target), A* counts expansions
(including the target and any reopens). Batch BFS counts nodes reached;
bidirectional Vertex BFS counts frontier nodes expanded from both ends,
so these BFS counts measure different work and should not be compared directly.
Canonical path reconstruction is excluded from the node count. The counters
already exist in the core; opting in only adds Python result conversion.

```python
measured = v.shortest_path("root", "target", method="dijkstra", stats=True)
assert measured.meta["stats"]["settled"] == measured.meta["expanded"]
```

#### Equal-cost paths

When several paths cost the same, every path method picks the same one each time: the choice depends only on the graph (nodes, edges, weights) and the node order, not on the order in which edges were added, the process (hash seeds) or the number of threads. A simulation that hashes or replays paths can rely on it.

**The rule:** walking back from the target, each step goes to the earliest neighbour, in node order, through which a cheapest route arrives.

- **Node order** is insertion order on a `Vertex` (the order of `keys()`; a node added after a removal can take the removed node's place) and the index (the position in `ids()`) on a `Projection`. `Vertex.project` numbers nodes in the Vertex's order, so `shortest_path`, `shortest_paths` and `project(...).shortest_paths` agree.
- `"bfs"` follows the rule with every edge counting 1, `"dijkstra"` with the weights. `k_shortest_paths` searches the same way: its first path is the one `shortest_paths` returns.
- `"astar"` applies the rule to the nodes it expands, in order of estimated total cost, then larger cost so far, then node order. It doesn't expand every node, so among equal-cost paths it can pick another one than `"dijkstra"`, but always the same one, and the same on a Vertex and on a Projection with the same coordinates and heuristic.
- Over a zero-weight edge, a route only replaces the one found first if it is strictly cheaper (re-linking at equal cost could close a cycle). Which route comes first is still fixed: nodes are settled by cost, ties in node order.
- Costs are compared exactly as computed (floating-point sums along the path), so two routes whose sums differ in the last bit are not tied.
- For a result that also doesn't depend on the order nodes were added, add them in a canonical order (for example sorted by id), or pass `ids` to `Projection.from_arrays` in that order; the order of the edges never matters.
- `Vertex(shuffle_seed=...)` doesn't change `shortest_path` (node order stays insertion order), but `project`, and so `Vertex.shortest_paths`, number the nodes in the shuffled order, so their ties can go another way.

```python
for edges in ([("a", "b"), ("b", "d"), ("a", "c"), ("c", "d")],
              [("c", "d"), ("a", "c"), ("b", "d"), ("a", "b")]):   # any edge order
    sq = Vertex()
    for n in ["a", "b", "c", "d"]:          # b before c: b wins the tie
        sq.add_node(n)
    for s, t in edges:
        sq.add_edge(s, t, {"weight": 1.0})
    assert sq.shortest_path("a", "d").meta["nodelist"] == ["a", "b", "d"]
    assert sq.shortest_path("a", "d", weight="weight").meta["nodelist"] == ["a", "b", "d"]
    assert sq.shortest_paths([("a", "d")])[0]["nodelist"] == ["a", "b", "d"]
```

#### A* — `method="astar"`

A* finds the same cheapest path as Dijkstra, but an estimate of the remaining distance to the target steers the search, so far fewer nodes are settled on spatial graphs (maps, grids, road networks). The estimate must never be larger than the real remaining cost (it may be smaller); otherwise the path returned may not be the cheapest. Tell it where the estimate comes from:

**Node coordinates** — `heuristic="euclidean"` (default) or `"manhattan"`, and `coords` says where each node keeps its coordinates:

| `coords` | Node attributes |
|---|---|
| `["x", "y"]` (default) | one attribute per dimension: `{"x": 1, "y": 2}` |
| `"pos"` | one attribute holding a tuple or list: `{"pos": (1, 2)}` |
| `["pos.lat", "pos.lon"]` | keys inside a dict attribute: `{"pos": {"lat": 1, "lon": 2}}` |

```python
grid = Vertex()
for x in range(10):
    for y in range(10):
        grid.add_node(f"{x},{y}", {"x": x, "y": y, "pos": (x, y)})
for x in range(10):
    for y in range(10):
        for nx_, ny_ in ((x + 1, y), (x, y + 1)):
            if nx_ < 10 and ny_ < 10:
                grid.add_edge(f"{x},{y}", f"{nx_},{ny_}", {"weight": 1.0})

path = grid.shortest_path("0,0", "9,9", heuristic="manhattan")      # coords=["x", "y"]
path.meta["cost"], path.meta["expanded"]                            # (18.0, 19)
path = grid.shortest_path("0,0", "9,9", method="astar", coords="pos")
grid.shortest_path("0,0", "9,9", method="dijkstra").meta["expanded"]  # 100
```

**Precomputed distances in the graph** — `distances="key"` reads `vertex.meta["key"]`, either `{node_id: estimate}` (estimates to one target) or `{node_id: {target_id: estimate}}` (to several targets). `vertex.meta` is live, so compute the table once and reuse it:

```python
grid.meta["to_corner"] = {
    n: (9 - grid[n].attr_get("x")) + (9 - grid[n].attr_get("y")) for n in grid.keys()
}
path = grid.shortest_path("0,0", "9,9", distances="to_corner")
```

Nodes without coordinates or without a table entry get estimate 0 (always safe, just less guided). The target itself must have coordinates.

#### Adding a path algorithm

Algorithms live in the pure-Rust core crate, `crates/ironweaver-core/src/pathfinding/`, one file each, registered in `METHODS` in `mod.rs`. A new one declares its name, description and options, and receives a validated `PathQuery` (direction, edge costs, `max_cost`, method options); the shared pieces — edge costs (`cost.rs`), heuristics (`heuristic.rs`) and best-first search (`best_first.rs`) — are reusable. The Python keyword options are parsed into the query in `src/vertex/pathfinding.rs`. See the comment at the top of the core `mod.rs`.

### Batch queries (parallel) — `vertex.shortest_paths(pairs, ...)`, `vertex.distances(sources, ...)`

For many queries at once. The graph's structure and edge costs are copied into a compact projection once (see below), then every query runs in parallel on all cores with the GIL released (other Python threads keep running). The snapshot costs one pass over the graph, so for a single query `shortest_path` is cheaper.

```python
res = v.shortest_paths([("root", "target"), ("root", "z"), ("z", "root")], weight="weight")
assert res[0]["nodelist"] == ["root", "a", "b", "target"]
assert abs(res[0]["cost"] - (0.9 + 0.4 + 0.8)) < 1e-9
assert res[2] is None                                  # unreachable: None, not an error

hops = v.shortest_paths([("root", "target")])          # method="bfs": number of edges
assert hops[0]["cost"] == 3

d = v.distances(["root", "a"], weight="weight", max_cost=1.5)
assert set(d["root"]) == {"root", "a", "b"}            # {source: {node: cost}}
assert v.distances(["root"], targets=["z"])["root"] == {"z": 2}
```

Paths are those `shortest_path` returns, ties included (see [Equal-cost paths](#equal-cost-paths)), whatever the number of threads. Both take `method` (`"bfs"` or `"dijkstra"`; None picks dijkstra if `weight` is given), `weight`, `default_weight`, `max_cost` and `direction`, like `shortest_path`. `shortest_paths` also takes `"astar"`, with `coords` (as in `shortest_path`; `["x", "y"]` by default), `heuristic` and `heuristic_scale` (see [A* on a projection](#a-on-a-projection)); every node needs coordinates. `distances` has no `"astar"`: an estimate towards one target does not help a search to every node. Edge weights are read and validated for the whole graph when the snapshot is built, so a negative or non-numeric weight anywhere raises when weights are supplied. Unknown node ids raise `ValueError`. The number of threads follows rayon (`RAYON_NUM_THREADS`).

### Projections — `vertex.project(...)`

Each `shortest_paths` / `distances` call copies the graph first. To run several batches (and, later, other analytics) on the same graph, copy it once into a `Projection`: a compact, read-only copy of the structure (sorted neighbour lists in both directions), at most one weight per edge and the node ids, but no attributes. Queries on it run in parallel with the GIL released, from any number of Python threads.

```python
p = v.project(weight="weight")                      # all nodes and edges, weighted
assert (p.node_count(), p.edge_count(), p.weighted) == (5, 4, True)
assert p.neighbors("a") == ["b", "z"] and p.neighbors("a", "in") == ["root"]
assert p.shortest_paths([("root", "target")])[0]["cost"] == res[0]["cost"]
assert p.distances(["root"], targets=["z"], method="bfs") == {"root": {"z": 2}}

knows = v.project(edge_filter={"type": "knows"}, direction="both")   # undirected, one edge type
assert sorted(knows.neighbors("a")) == ["root", "z"]
sub = v.project(nodes=["root", "a", "b"], node_filter=lambda n: n.id != "b")
assert sub.ids() == ["root", "a"]
```

`project(weight=None, default_weight=None, *, direction=None, nodes=None, node_filter=None, edge_filter=None, coords=None)`:

- Without `weight` and `default_weight` the projection is unweighted (only `"bfs"` queries).
- `direction`: `"out"` keeps edges as they are, `"in"` reverses them, `"both"` makes them undirected.
- `nodes` limits the projection to those ids.
- `node_filter` / `edge_filter` take a dict (attribute equality) or a callable receiving a `NodeView` / `EdgeView`. The edge filter is asked once per edge between kept nodes.
- Weights are read and validated for the kept edges only.
- `coords` compiles node coordinates in, for `"astar"`: an attribute holding a sequence (`"pos"`) or one attribute path per dimension (`["x", "y"]`, `["pos.lat", "pos.lon"]`). Every kept node needs them, all of the same length; a missing one raises `ValueError`, a non-numeric one `TypeError`.

A projection is a snapshot: later changes to the graph don't affect it. Don't change the graph from inside a filter.

Projection methods:

- `shortest_paths(pairs, method=None, *, max_cost=None, heuristic=None, heuristic_scale=None, stats=False)` and `distances(sources, targets=None, method=None, *, max_cost=None)` work as on `Vertex`. `method=None` picks `"astar"` if `heuristic` or `heuristic_scale` is given, `"dijkstra"` on a weighted projection and `"bfs"` otherwise.
- `neighbors(id, direction="out")`, `degree(id, direction="out")`: `"out"` or `"in"`, one entry per edge.
- `ids()`, `node_count()`, `edge_count()`, `len(p)`, `id in p`, `weighted`, `direction`, `coord_dim` (coordinates per node, 0 without), `memory_usage()` (bytes).
- `set_weights(edges, weights)` and `edge_index(edge)` change edge weights in place (see [Changing weights](#changing-weights--set_weights)).

#### A* on a projection

With coordinates compiled in (`project(coords=...)` or `Projection.from_arrays(..., coords=...)`), `shortest_paths(pairs, "astar")` guides each search towards its target. The estimate of the remaining cost is `heuristic_scale` (default 1) times the `"euclidean"` (default) or `"manhattan"` distance between the node's and the target's coordinates, read from a flat array.

```python
g = Vertex()
for x in range(4):
    for y in range(3):
        g.add_node(f"{x},{y}", {"x": x, "y": y})
for x in range(4):
    for y in range(3):
        for nx, ny in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]:
            if 0 <= nx < 4 and 0 <= ny < 3:
                g.add_edge(f"{x},{y}", f"{nx},{ny}", {"cost": 2.0 if ny == 1 else 1.0})  # entering row 1 costs 2

grid = g.project(weight="cost", coords=["x", "y"])
fast = grid.shortest_paths([("0,0", "3,2")], "astar", heuristic="manhattan")
assert fast[0]["cost"] == grid.shortest_paths([("0,0", "3,2")], "dijkstra")[0]["cost"] == 6.0
assert g.shortest_paths([("0,0", "3,2")], "astar", weight="cost", heuristic="manhattan") == fast
```

- The path is the cheapest one when the estimate never exceeds the real remaining cost. That holds when every edge costs at least `heuristic_scale` times the distance between its ends: on a grid with unit steps and costs of at least 1 per tile, `heuristic="manhattan"` qualifies; if steps cost at least `c`, `heuristic_scale=c` finds the same paths and settles fewer nodes. A larger scale searches less but may return a more expensive path. Nodes reached again more cheaply are expanded again, so an admissible estimate is enough (it need not be consistent).
- The distance is symmetric, so `"in"` (reversed) and `"both"` projections work the same way.
- `"astar"` needs a weighted projection with coordinates; `distances` does not take it.
- On a 180,000-tile grid (`benchmarks/compare_astar.py`), A* with the Manhattan distance settles a fraction of the nodes Dijkstra does and is several times faster per trip.

#### Without a Vertex — `Projection.from_arrays(...)`

When the graph is already in arrays (node ids, edge sources and targets as indices into the ids, weights), build the projection directly. It skips the Vertex (about 1.4 KB per node, against under 100 bytes for the projection) and is many times faster: on a 180,000-node, 700,000-edge grid it takes about 60 ms, against about 1.8 s for `add_nodes` + `add_edges` + `project`.

```python
import numpy as np
from ironweaver import Projection

ids = ["a", "b", "c", "d"]
src = np.array([0, 0, 1, 2], dtype=np.int64)       # edge k: ids[src[k]] -> ids[dst[k]]
dst = np.array([1, 2, 3, 3], dtype=np.int64)
weight = np.array([1.0, 2.0, 5.0, 1.0])
p = Projection.from_arrays(ids, src, dst, weight)
assert p.shortest_paths([("a", "d")]) == [{"nodelist": ["a", "c", "d"], "cost": 3.0}]
undirected = Projection.from_arrays(ids, src, dst, direction="both")   # unweighted
assert undirected.neighbors("d") == ["b", "c"]
```

`Projection.from_arrays(ids, src, dst, weight=None, coords=None, *, direction=None)`:

- Node `i` is `ids[i]` (ids must be distinct). Edge `k` goes from `ids[src[k]]` to `ids[dst[k]]` with weight `weight[k]`; without `weight` the projection is unweighted.
- Edges keep the order given, so the result is exactly what `project(weight=..., direction=...)` gives on a Vertex with the same nodes and edges added in the same order. Equal-cost paths are chosen by node index (see [Equal-cost paths](#equal-cost-paths)), so the order of the edges doesn't change any path; order `ids` canonically for a canonical choice.
- `src` / `dst` are 1-D int64 or int32 numpy arrays, `weight` float64 or float32 (any object with the buffer protocol; read without a Python call per item), or plain sequences. numpy itself is not required.
- `coords` (an `(n, d)` array or `n` rows of `d` numbers) are node coordinates, for [A*](#a-on-a-projection).
- Raises `ValueError` for lists of different lengths, indices out of range, negative or NaN weights (as on Vertex edges), duplicate ids or coords that are not finite or not one row per node.

#### Changing weights — `set_weights(...)`

The structure of a projection is fixed, but its weights can change in place: `set_weights(edges, weights)` gives edge `edges[i]` the weight `weights[i]`. It costs time in proportion to the number of edges changed, not to the size of the projection: on a 180,000-tile grid (`benchmarks/compare_set_weights.py`), changing the 4 edges into a tile takes about a microsecond, against about 60 ms to rebuild the projection. An infinite weight makes an edge impassable, a finite one makes it passable again, so a game map can block tiles (a building) or make them cheaper (a road) between queries.

```python
w, h = 5, 3
ids = [f"{x},{y}" for y in range(h) for x in range(w)]
src, dst = [], []
for y in range(h):
    for x in range(w):
        for nx, ny in [(x + 1, y), (x, y + 1)]:
            if nx < w and ny < h:
                a, b = y * w + x, ny * w + nx
                src += [a, b]
                dst += [b, a]
src, dst = np.array(src), np.array(dst)
tiles = Projection.from_arrays(ids, src, dst, np.ones(len(src)))
assert tiles.shortest_paths([("0,1", "4,1")])[0]["cost"] == 4.0

building = np.flatnonzero(dst == 1 * w + 2)        # the edges into tile 2,1
tiles.set_weights(building, np.full(len(building), np.inf))
path = tiles.shortest_paths([("0,1", "4,1")])[0]
assert path["cost"] == 6.0 and "2,1" not in path["nodelist"]
tiles.set_weights(building, np.ones(len(building)))  # demolished
assert tiles.shortest_paths([("0,1", "4,1")])[0]["cost"] == 4.0

# A Vertex projection numbers its edges for you
v2 = Vertex()
for n in "abc":
    v2.add_node(n)
ab = v2.add_edge("a", "b", {"weight": 1.0})
v2.add_edge("b", "c", {"weight": 1.0})
v2.add_edge("a", "c", {"weight": 5.0})
vp = v2.project(weight="weight")
vp.set_weights([vp.edge_index(ab)], [np.inf])
assert vp.shortest_paths([("a", "c")])[0]["nodelist"] == ["a", "c"]
```

- **Edge numbers** are positions in projected order. For `from_arrays` that is the order of the arrays: edge `k` is `src[k] -> dst[k]`. For `Vertex.project` it is the order in which the Vertex stores its edges (insertion order, except that an edge added after a removal can take the removed edge's place), counting only the projected edges; `edge_index(edge)` returns an `Edge`'s number (weighted projections made by `Vertex.project` only; `ValueError` for an edge of another Vertex, or one not projected).
- One edge has one number in every direction: on a `"both"` projection it is listed from both of its ends and both entries change, and the reversed adjacency (`neighbors(id, "in")`) changes too. If an edge is listed twice, the last weight wins.
- Afterwards every method returns exactly what a projection built with the new weights returns.
- **`inf` is impassable** everywhere a path is searched, with weights or with hop counts: `shortest_paths` and `distances` (any method), `bfs_levels`, `k_shortest_paths`, betweenness, closeness and harmonic centrality, node2vec walks; `minimum_spanning_tree` leaves such edges out, and PageRank, FastRP, Leiden and modularity count them as weight 0. Methods that only look at the structure (components, `topological_sort`, `find_cycle`, `degree_centrality`, triangles, clustering, `core_number`, `label_propagation`, similarity, `neighbors`, `degree`) ignore weights and still count them.
- Raises `ValueError` (changing nothing) on an unweighted projection, for lists of different lengths, an index outside `0 .. edge_count() - 1`, or a negative or NaN weight. `edges` / `weights` are arrays or sequences, as for `from_arrays`.
- A query running in another thread holds the projection: `set_weights` raises `RuntimeError` meanwhile instead of changing weights under it.
- A weighted projection keeps 4 bytes per edge (8 for `"both"`) to find each edge's entries, and once the reversed adjacency is built 4 more per entry; a weighted Vertex projection also keeps its edges' handles (8 bytes each).

### Random walks — `vertex.random_walks(...)`

Generate multiple random walks from a starting node.

```python
walks = v.random_walks(
    start_node_id="a",
    max_length=10,
    num_attempts=100,
    min_length=3,          # optional, default 1
    allow_revisit=False,   # optional, default False
    include_edge_types=True,  # optional, default False
    edge_type_field="type",   # optional, default "type"
    seed=42,               # optional, makes the walks reproducible
)
# walks is a list of lists, e.g. [["a", "knows", "b", "follows", "c"], ...]
```

Duplicate walks are automatically removed.
