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

### Adding many at once

```python
bulk = Vertex()
bulk.add_nodes(["n1", "n2", ("n3", {"age": 7})], labels=["Item"])     # ids or (id, attrs)
bulk.add_edges([("n1", "n2"), ("n2", "n3", {"note": "x"})], type="next")  # (from, to) or (from, to, attrs)
bulk.add_edges([("n3", "n1"), ("n1", "n3")], attrs={"weight": [0.5, 2.0], "type": ["back", None]})
assert bulk.node_count() == 3 and bulk.project().edge_count() == 4
```

`add_nodes(nodes, *, labels=None, attrs=None)` and `add_edges(edges, *, type=None, attrs=None)` add a whole batch in one call. They're the fast way to load a large graph:
- Every item is checked before anything is added, so an invalid item (an unknown or duplicate id, a wrong shape) leaves the graph unchanged.
- `attrs` gives attributes as columns: `{name: [one value per item]}`. `None` leaves that attribute out for the item. A `"labels"` column (nodes) or `"type"` column (edges) sets the field. Columns avoid creating a Python dict per item, so they are the fastest way to load weights.
- They return the number of items added. Add-callbacks fire once per item after the whole batch is in.

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
assert v.labels()["Admin"] == 1          # every label, with its number of nodes

assert e.type == "knows"
assert v.edge_type_count("knows") >= 1   # counted as edges change, O(1)
print(v.edge_types())                    # {type (None: untyped): number of edges}
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

### Indexes

`find(name, value)` and `find_range(name, low, high)` look nodes up by an attribute. They scan every node unless the attribute is indexed:

```python
from ironweaver import Vertex, attr

g = Vertex()
g.add_nodes([("ann", {"age": 31, "city": "Oslo"}), ("bob", {"age": 25, "city": "Rome"}),
             ("cat", {"age": 40, "city": "Oslo"})])
g.create_index("city")                                  # returns False if it exists
g.create_index("age")
assert g.indexes == ["city", "age"]
assert [n.id for n in g.find("city", "Oslo")] == ["ann", "cat"]
assert [n.id for n in g.find_range("age", 30, 40, inclusive="left")] == ["ann"]
g["bob"].attr_set("city", "Oslo")                       # indexes follow every change
assert [n.id for n in g.find("city", "Oslo")] == ["ann", "bob", "cat"]
assert sorted(g.filter(where=attr("age") > 30).nodes) == ["ann", "cat"]   # uses the index
stats = g.index_stats("city")                          # None if not indexed
assert (stats["entries"], stats["distinct_keys"], stats["dirty"]) == (3, 1, 0)
assert stats["memory_bytes"] <= g.memory_usage()
g.drop_index("city")
```

- Results are in graph order. Numbers compare across int and float (`find("age", 31.0)` finds 31).
- `find_range` takes `low` and/or `high` (None: open) of one kind (numbers, strings, bools, bytes, dates or datetimes); values of other kinds are never in range. `inclusive` is `"both"` (default), `"left"`, `"right"` or `"neither"`.
- `index_stats(name)` gives an index's size in O(1): `entries` (indexed nodes), `distinct_keys`, `memory_bytes` (its share of `memory_usage()`) and `dirty` (nodes whose entries may be stale; 0 after changes made through the graph).
- Only scalar values are indexed; lists, dicts, None and NaN never match, as in filter expressions.
- `filter(where=...)` and `match` use indexes on the attributes they compare (equality, ranges, `is_in`), and `match` starts from the pattern node with the fewest candidates.
- Indexes follow changes made through the graph (`add_node(s)`, `attr_set`, assigning `attr`, `remove_node`). Changing a list or dict *inside* an attribute value in place doesn't count, but such values aren't indexed anyway.
- Indexes are saved with the graph (which attributes are indexed, not the index contents): loading a file rebuilds them.

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

Files are written in format version 2: node labels, edge types and edge ids are saved; binary files carry a header and a checksum (truncated or corrupted files are rejected). JSON files from older versions still load (binary files saved by 0.1 don't; see [the file format docs](format.md#format-1-ironweaver-01) to convert them). There, edges get new ids, the old string id is kept in `edge.meta["legacy_id"]`, and `attr["labels"]` / `attr["type"]` become labels and types.

Attribute values can be None, bools, ints, floats, strings, bytes, `datetime.date` / `datetime.datetime` (aware ones keep their UTC offset), and lists / dicts of those; they load back with the same types. numpy arrays and scalars are saved as lists / numbers, anything else as its `str()`.

```python
import datetime as dt
import os
import tempfile
from ironweaver import Vertex

g = Vertex()
g.add_node("launch", {"day": dt.date(1969, 7, 16), "raw": b"\x00\x01",
                      "at": dt.datetime(1969, 7, 16, 13, 32, tzinfo=dt.timezone.utc)})
path = os.path.join(tempfile.mkdtemp(), "g.bin")
g.save_to_binary(path)
attr = Vertex.load_from_binary(path)["launch"].attr
assert attr["day"] == dt.date(1969, 7, 16) and attr["raw"] == b"\x00\x01"
assert attr["at"].tzinfo is not None
```

Saving to a file is atomic: the graph is written to a temporary file next to the target and renamed over it, so a failed or interrupted save never leaves a half-written file (the previous file stays as it was). Attribute values may nest lists and dicts at most 100 levels deep; saving deeper values (or a list that contains itself) and loading files with deeper values raise `RuntimeError`.

### Metadata & analysis

```python
v.meta["project"] = "demo"
v.get_metadata()      # dict with node_count, edge_count, etc.
v.memory_usage()      # bytes used by the structure (ids, adjacency, labels, indexes)
v.memory_usage(deep=True)  # plus the attribute dicts and their values
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
