# Callbacks

Ironweaver supports callbacks on a `Vertex` to observe graph mutations in real time.

## Callback Types

| List on Vertex | Signature | Fired when |
|---|---|---|
| `on_node_add_callbacks` | `(vertex, node) -> bool?` | A node is added via `add_node` |
| `on_edge_add_callbacks` | `(vertex, edge) -> bool?` | An edge is added via `add_edge` |
| `on_node_update_callbacks` | `(vertex, node, key, new_value, old_value) -> bool?` | A node attribute changes via `node.attr_set()` |
| `on_edge_update_callbacks` | `(vertex, edge, key, new_value, old_value) -> bool?` | An edge attribute changes via `edge.attr_set()` |

All callbacks are stored as Python lists on the `Vertex`. Return `False` from any callback to stop subsequent callbacks in the same list from firing.

## Usage

```python
from ironweaver import Vertex

v = Vertex()

# Listen for new nodes
def on_add(vertex, node):
    print(f"added {node.id}")
v.on_node_add_callbacks.append(on_add)

# Listen for attribute changes on nodes
def on_update(vertex, node, key, new_val, old_val):
    print(f"{node.id}.{key}: {old_val} -> {new_val}")
v.on_node_update_callbacks.append(on_update)

n = v.add_node("a")        # prints: added a
n.attr_set("color", "red") # prints: a.color: None -> red
n.attr_set("color", "red") # no output — value unchanged
n.attr_set("color", "blue")# prints: a.color: red -> blue
```

## How It Works

- `node.on_update_callbacks` / `edge.on_update_callbacks` *are* the vertex's `on_node_update_callbacks` / `on_edge_update_callbacks` lists. Callbacks appended to `vertex.on_node_update_callbacks` later automatically apply to all existing nodes.
- `attr_set` compares the new value against the existing one using Python equality (`==`). Callbacks only fire when the value actually changes.
- `old_value` is `None` when the key did not previously exist.
