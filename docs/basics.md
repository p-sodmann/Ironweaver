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

### Metadata & analysis

```python
v.meta["project"] = "demo"
v.get_metadata()      # dict with node_count, edge_count, etc.
G = v.to_networkx()   # convert to networkx.DiGraph
```

---

## Node

A node has an `id`, a dict of `attr`, outgoing `edges`, and `inverse_edges`.

```python
node = v.add_node("x", attr={"label": "hello"})

node.id                      # "x"
node.attr                    # {"label": "hello"}
node.attr_get("label")       # "hello"
node.attr_set("label", "hi") # fires on_node_update_callbacks
node.attr_list_append("tags", "new")

node.edges                   # outgoing edges
node.inverse_edges           # incoming edges
node.vertex                  # back-reference to the owning Vertex
```

---

## Edge

An edge connects two nodes and carries its own `attr` dict.

```python
e = v.add_edge("x", "y", attr={"type": "follows", "weight": 1.0})

e.from_node                  # Node "x"
e.to_node                    # Node "y"
e.attr                       # {"type": "follows", "weight": 1.0}
e.attr_get("type")           # "follows"
e.attr_set("weight", 2.0)   # fires on_edge_update_callbacks
e.vertex                     # back-reference to the owning Vertex
```
