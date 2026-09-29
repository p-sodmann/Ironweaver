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
b = v.add_node("b", attr={"color": "red"}, labels=["Person"])
```

### Adding edges

```python
e = v.add_edge("a", "b", type="knows")   # or attr={"type": "knows"}
```

### Labels, edge types and edge ids

Nodes carry a set of **labels** and edges a **type**. Both are graph fields:
the graph indexes labels, and filters and algorithms can match them without
reading attributes. Every edge has a persistent integer **id**. Ids are
unique in the graph and never reused, even after the edge is removed. They
are kept when saving and loading, and when deriving graphs with `filter` or
`bfs`.

```python
assert b.labels == ["Person"] and b.has_label("Person")
a.add_label("Admin")
assert [n.id for n in v.nodes_with_label("Admin")] == ["a"]

assert e.type == "knows"
assert v.get_edge(e.id) == e             # look an edge up by id
```

`"labels"` and `"type"` still work as attribute names:
- `add_node(id, {"labels": [...]})` sets the labels, and `add_edge(a, b, {"type": "knows"})` sets the type (only for a list of str / a str; other values stay ordinary attributes).
- `node.attr` / `edge.attr` show them under those keys.
- `attr_get` / `attr_set` and assigning `attr = {...}` read and write the fields.
- Traversal and projection dict filters (`{"type": "knows"}`) match the fields.

A node's `attr["type"]` is an ordinary attribute, since nodes have labels, not a type.

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

Files are written in format version 2: node labels, edge types and edge ids are saved; binary files carry a header and a checksum (truncated or corrupted files are rejected). Files from older versions still load. There, edges get new ids, the old string id is kept in `edge.meta["legacy_id"]`, and `attr["labels"]` / `attr["type"]` become labels and types.

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
e.type                       # "follows"
e.id                         # persistent integer id
e.attr                       # {"weight": 1.0, "type": "follows"}
e.attr_get("type")           # "follows"
e.attr_set("weight", 2.0)   # fires on_edge_update_callbacks
e.vertex                     # back-reference to the owning Vertex
```
