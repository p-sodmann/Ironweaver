# Vertex, Node & Edge

## Vertex

A `Vertex` is a directed graph — a collection of nodes connected by edges.

```python
from ironweaver import Vertex

v = Vertex()
```

### Adding nodes

```python
a = v.add_node("a")
b = v.add_node("b", attr={"color": "red"})
```

### Adding edges

```python
e = v.add_edge("a", "b", attr={"type": "knows"})
```

### Querying

```python
v.has_node("a")       # True
v.node_count()        # 2
v.keys()              # ["a", "b"]
node = v.get_node("a")
node = v["a"]         # same thing
```

### Serialization

```python
# JSON
v.save_to_json("graph.json")
v2 = Vertex.load_from_json("graph.json")

# Binary (faster for large graphs)
v.save_to_binary("graph.bin")
v2 = Vertex.load_from_binary("graph.bin")

# Binary with f16 precision (smaller files)
v.save_to_binary_f16("graph_f16.bin")
```

Saving to a file is atomic: the graph is written to a temporary file next to the target and renamed over it, so a failed or interrupted save never leaves a half-written file (the previous file stays as it was). Attribute values may nest lists and dicts at most 100 levels deep; saving deeper values (or a list that contains itself) and loading files with deeper values raise `RuntimeError`.

### Metadata & analysis

```python
v.meta["project"] = "demo"
v.get_metadata()      # dict with node_count, edge_count, etc.
G = v.to_networkx()   # convert to networkx.DiGraph
```

---

## Node

A node has an `id`, a dict of `attr`, outgoing `edges`, and `inverse_edges`.

> **Copies, not live views:** `node.attr`, `node.meta`, `node.edges` and
> `node.inverse_edges` (and `edge.attr`, `edge.meta`, `vertex.nodes`) return
> a *copy* each time. `node.attr["k"] = v` is silently lost — use
> `node.attr_set("k", v)` or assign a whole dict (`node.attr = {...}`).
> `vertex.meta` and the `on_*_callbacks` lists, in contrast, are live.

> **Handles:** the graph data lives in Rust and a `Node` (or `Edge`) is a
> handle to one node of its `vertex`. `v["x"] == v["x"]`, but the two may be
> different objects, so compare with `==` rather than `is`.

```python
node = v.add_node("x", attr={"label": "hello"})

node.id                      # "x"
node.attr                    # {"label": "hello"}
node.attr_get("label")       # "hello"
node.attr_set("label", "hi") # fires on_node_update_callbacks
node.attr["label"] = "lost"  # no effect: node.attr is a copy
node.attr_list_append("tags", "new")

node.edges                   # outgoing edges
node.inverse_edges           # incoming edges
node.vertex                  # the owning Vertex
assert v["x"] == node       # handles compare by node
```

---

## Edge

An edge connects two nodes and carries its own `attr` dict.

```python
v.add_node("y")
e = v.add_edge("x", "y", attr={"type": "follows", "weight": 1.0})

e.from_node                  # Node "x"
e.to_node                    # Node "y"
e.attr                       # {"type": "follows", "weight": 1.0}
e.attr_get("type")           # "follows"
e.attr_set("weight", 2.0)   # fires on_edge_update_callbacks
e.vertex                     # back-reference to the owning Vertex
```
