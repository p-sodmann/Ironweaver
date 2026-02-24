# Filtering

All filter operations return a **new** `Vertex` containing only the matched nodes and the edges between them. The original graph is unchanged.

## Lambda filtering

Pass a callable that receives a `NodeView` and returns `True` for nodes to keep.

```python
sub = v.filter(lambda n: (
    n.id.startswith("test_")
    and n.type in {"A", "B", "C"}
    and n.attr("score") < 0.8
    and n.attr("status") != "archived"
))
```

### NodeView API

The `NodeView` passed to your predicate exposes:

| Property / Method | Description |
|---|---|
| `n.id` | Node identifier |
| `n.type` | Shortcut for `attr["type"]` |
| `n.attr("key")` | Attribute value, or `None` if missing |
| `n.attr("key", default)` | Attribute value with fallback |
| `n.has_attr("key")` | `True` if the attribute exists |
| `n.attrs` | Full attribute dict |
| `n.edges` | Outgoing edges |
| `n.inverse_edges` | Incoming edges |
| `n.degree` / `n.in_degree` | Outgoing / incoming edge count |
| `n.has_edge_to("id")` | `True` if an outgoing edge reaches that node |
| `n.has_edge_from("id")` | `True` if an incoming edge comes from that node |
| `n.neighbor_ids` | Set of outgoing neighbour IDs |
| `n.node` | The underlying `Node` object |

### Predicate helpers

Combinators are available in `ironweaver.filter.predicates`:

```python
from ironweaver.filter.predicates import attr_equals, attr_contains, p_and, p_or, p_not

sub = v.filter(p_and(
    attr_contains("Labels", "Field"),
    p_not(attr_equals("type", "selector"))
))
```

## Filter by ID

```python
sub = v.filter(id="a")
sub = v.filter(ids=["a", "b", "c"])
```

## Filter by attribute

Pass keyword arguments to match nodes whose `attr` contains that key/value pair. All conditions must match (AND logic).

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
