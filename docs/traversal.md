# Traversal

Ironweaver provides several traversal methods on both `Node` and `Vertex`.

## Node-level traversal

All node-level methods start from a single node and follow outgoing edges. They return a `Vertex` with the discovered nodes and a `meta["nodelist"]` recording visit order.

### DFS — `node.traverse(depth, filter)`

```python
node = v.get_node("root")
result = node.traverse(depth=3)
result.meta["nodelist"]  # visit order
```

### BFS — `node.bfs(depth, filter)`

```python
result = node.bfs(depth=2)
```

### BFS search — `node.bfs_search(target_id, depth, filter)`

Returns the target `Node` if reachable, otherwise `None`. Stops as soon as the target is found.

```python
found = node.bfs_search("target", depth=5)
```

### Edge filtering

All three methods accept an optional `filter` dict to restrict which edges are followed:

```python
result = node.bfs(depth=2, filter={"type": "knows"})
```

Only edges whose `attr` matches **every** key/value pair in the filter are traversed.

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
