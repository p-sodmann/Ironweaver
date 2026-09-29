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

With `method=None` it picks `"astar"` if `heuristic`, `coords` or `distances` is given, `"dijkstra"` if `weight` is given, and `"bfs"` otherwise. Options shared by the weighted methods: `weight` (edge attribute, default `"weight"`), `default_weight` (cost of edges without it, default 1.0), `max_cost`, and `direction` (`"out"`, `"in"`, `"both"`). An option a method doesn't accept raises `TypeError`.

The result is a new `Vertex` with copies of the path's nodes and the edges between them. `meta["nodelist"]` has the path in order, `meta["cost"]` its cost (the number of edges for `"bfs"`), `meta["method"]` the algorithm used, and `meta["expanded"]` how many nodes Dijkstra/A* settled. `ValueError` if a node is missing or the target is unreachable. If several shortest paths exist, one of them is returned.

```python
path = v.shortest_path("root", "target")                     # bfs: fewest edges
path.meta["nodelist"]                                        # ['root', 'a', 'b', 'target']
path = v.shortest_path("root", "target", weight="weight")    # dijkstra: cheapest
path.meta["cost"]                                            # 2.1
path = v.shortest_path("target", "root", method="bfs", direction="in", max_depth=10)
```

`shortest_path_bfs(root, target, max_depth, direction)` and `shortest_path_dijkstra(root, target, weight, default_weight, max_cost, direction)` are shorthands for `method="bfs"` and `method="dijkstra"`.

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

For many queries at once. The graph's structure and edge costs are copied into a compact snapshot once, then every query runs in parallel on all cores with the GIL released (other Python threads keep running). The snapshot costs one pass over the graph, so for a single query `shortest_path` is cheaper.

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

Both take `method` (`"bfs"` or `"dijkstra"`; None picks dijkstra if `weight` is given), `weight`, `default_weight`, `max_cost` and `direction`, like `shortest_path`. `"astar"` is not available here. Edge weights are read and validated for the whole graph when the snapshot is built, so a negative or non-numeric weight anywhere raises (for weighted methods). Unknown node ids raise `ValueError`. The number of threads follows rayon (`RAYON_NUM_THREADS`).

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
