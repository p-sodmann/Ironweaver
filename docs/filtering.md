# Filtering

All filter operations return a **new** `Vertex` containing only the matched nodes and the edges between them. The original graph is unchanged.

## Filter by ID

```python
# Single node
sub = v.filter(id="a")

# Multiple nodes
sub = v.filter(ids=["a", "b", "c"])
```

## Filter by attribute

Pass any keyword argument to match nodes whose `attr` contains that key/value pair. All conditions must match (AND logic).

```python
sub = v.filter(color="red")
sub = v.filter(color="red", status="active")
```

## Expand

Grow an existing sub-graph by pulling in neighbours from a larger source graph.

```python
full = Vertex.load_from_json("big_graph.json")
seed = full.filter(id="start_node")

# Add direct neighbours (depth=1, the default)
expanded = seed.expand(full)

# Go two hops out
expanded = seed.expand(full, depth=2)
```

`expand` performs a BFS from every node in the current vertex into `source_vertex` up to the given depth, then returns a new vertex with all discovered nodes and the edges between them.
