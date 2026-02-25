# Traversal

Ironweaver provides several traversal methods on both `Node` and `Vertex`.

## Node-level traversal

All node-level methods start from a single node and follow outgoing edges. They return a `Vertex` with the discovered nodes and a `meta["nodelist"]` recording visit order.

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

### Shortest path — `vertex.shortest_path_bfs(root, target, max_depth)`

Returns a new `Vertex` containing only the nodes along the shortest path.

```python
path = v.shortest_path_bfs("a", "z")
path = v.shortest_path_bfs("a", "z", max_depth=10)
```

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
)
# walks is a list of lists, e.g. [["a", "knows", "b", "follows", "c"], ...]
```

Duplicate walks are automatically removed.
