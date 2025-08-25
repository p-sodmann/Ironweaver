# LGF (Lemon Graph Format) Documentation

The Lemon Graph Format (LGF) is a human-readable text format for representing graphs with nodes, edges, and attributes. IronWeaver provides comprehensive support for parsing LGF files with both programmatic and file-based interfaces.

## Quick Start

```python
from ironweaver import parse_lgf, parse_lgf_file

# Parse from string
graph = parse_lgf(lgf_text)

# Parse from file
graph = parse_lgf_file("graph.lgf")
```

## LGF Syntax

### Node Declaration

Nodes are declared with an identifier followed by optional labels:

```lgf
node_id Label1 Label2
  attribute1 = "value"
  attribute2 = 42
  attribute3 = true
```

**Example:**
```lgf
alice Person Employee
  name = "Alice Johnson"
  age = 30
  is_manager = true
  salary = 75000
```

### Edge Declaration

IronWeaver supports the modern LGF syntax for edge declarations:

#### Forward Relationships

Create an edge from the current node to a target node:

```lgf
node_id NodeType
  -relationship_type-> target_node
```

**Example:**
```lgf
alice Person
  name = "Alice"
  -knows-> bob
  -works_at-> tech_corp
  -manages-> charlie
    start_date = "2023-01-15"
```

#### Inverse Relationships

Create an edge from a target node to the current node:

```lgf
node_id NodeType
  <-relationship_type- source_node
```

**Example:**
```lgf
alice Person
  name = "Alice"
  <-reports_to- bob
  <-founded_by- tech_corp
```

### Edge Attributes

Edges can have attributes specified with indented lines after the edge declaration:

```lgf
alice Person
  name = "Alice"
  -knows-> bob
    since = "2020-03-15"
    strength = 0.8
    context = "work"
```

### Data Types

LGF supports several data types with automatic parsing:

#### Strings
```lgf
name = "Alice Johnson"        # Double quotes
title = 'Software Engineer'   # Single quotes
description = Unquoted text   # Unquoted (treated as string)
```

#### Numbers
```lgf
age = 30           # Integer
salary = 75000.50  # Float
score = -5         # Negative numbers
```

#### Booleans
```lgf
is_active = true
is_deleted = false
```

#### Special Values
Empty or missing values are handled gracefully:
```lgf
optional_field =    # Empty value (empty string)
```

## Advanced Features

### Import Support

LGF files can import other LGF files using the `import()` directive:

```lgf
import("shared_nodes.lgf")
import("relationships.lgf")

# Additional nodes and edges specific to this file
alice Person
  name = "Alice"
  -knows-> bob  # bob might be defined in shared_nodes.lgf
```

**Import Rules:**
- Import paths are relative to the importing file
- Imported files are processed before the current file content
- Nodes defined in imported files can be referenced in the current file
- If a node is defined in multiple files, attributes are merged

### Complex Example

Here's a comprehensive example showing various LGF features:

```lgf
# Define persons
alice Person Employee
  name = "Alice Johnson"
  age = 30
  department = "Engineering"
  is_manager = true
  -knows-> bob
    since = "2020-01-15"
    relationship_type = "colleague"
  -knows-> charlie
    since = "2019-06-20"
    relationship_type = "friend"
  -manages-> charlie
    start_date = "2022-03-01"
  -works_at-> tech_corp
    position = "Senior Developer"
    start_date = "2019-01-10"

bob Person Employee
  name = "Bob Smith"
  age = 28
  department = "Engineering"
  is_manager = false
  -works_at-> tech_corp
    position = "Developer"
    start_date = "2020-01-15"
  <-reports_to- alice

charlie Person Employee
  name = "Charlie Brown"
  age = 26
  department = "Engineering"
  is_manager = false
  -works_at-> tech_corp
    position = "Junior Developer"
    start_date = "2021-03-01"

tech_corp Company
  name = "Tech Corporation"
  founded = "2015-05-20"
  industry = "Software"
  size = "medium"
  <-founded_by- alice
    role = "Co-founder"
    equity = 0.25
```

## Python API

### Function Reference

#### `parse_lgf(text, graph=None, base_path=None)`

Parse LGF content from a string.

**Parameters:**
- `text` (str): LGF content as string
- `graph` (Vertex, optional): Existing graph to add nodes/edges to
- `base_path` (str, optional): Base path for resolving import statements

**Returns:**
- `Vertex`: The parsed graph

**Example:**
```python
from ironweaver import parse_lgf

lgf_content = """
alice Person
  name = "Alice"
  -knows-> bob

bob Person
  name = "Bob"
"""

graph = parse_lgf(lgf_content)
print(f"Nodes: {list(graph.keys())}")  # ['alice', 'bob']
```

#### `parse_lgf_file(path)`

Parse LGF content from a file.

**Parameters:**
- `path` (str): Path to the LGF file

**Returns:**
- `Vertex`: The parsed graph

**Example:**
```python
from ironweaver import parse_lgf_file

graph = parse_lgf_file("my_graph.lgf")
print(f"Loaded {graph.node_count()} nodes")
```

### Working with Parsed Graphs

Once parsed, LGF graphs are standard IronWeaver `Vertex` objects with full API support:

```python
# Parse LGF
graph = parse_lgf_file("social_network.lgf")

# Access nodes
alice = graph.get_node("alice")
print(f"Alice's attributes: {dict(alice.attr)}")

# Access edges
for edge in alice.edges:
    print(f"Alice {edge.attr['type']} {edge.to_node.id}")

# Use graph algorithms
shortest_path = graph.shortest_path_bfs("alice", "charlie")
print(f"Path from Alice to Charlie: {list(shortest_path.keys())}")

# Convert to NetworkX for visualization
import networkx as nx
import matplotlib.pyplot as plt

nx_graph = graph.to_networkx()
pos = nx.spring_layout(nx_graph)
nx.draw(nx_graph, pos, with_labels=True, node_color='lightblue')
plt.show()
```

## Migration Notes

### Deprecated Syntax

The old LGF syntax `-> target relationship` is no longer supported as of version X.X.X. Please update your LGF files to use the new syntax:

**Old (Deprecated):**
```lgf
alice Person
  -> bob knows
```

**New (Supported):**
```lgf
alice Person
  -knows-> bob
```

### Breaking Changes

- **Version X.X.X**: Removed support for old arrow syntax (`-> target relationship`)
- The new syntax provides better readability and consistency
- Edge attributes are now properly supported with the new syntax

## Best Practices

### File Organization

1. **Use imports** for shared node definitions:
   ```lgf
   # people.lgf
   alice Person
     name = "Alice"
   
   bob Person  
     name = "Bob"
   ```

   ```lgf
   # relationships.lgf
   import("people.lgf")
   
   alice Person
     -knows-> bob
   ```

2. **Group related nodes** together in the same file
3. **Use meaningful node IDs** that reflect the domain

### Naming Conventions

1. **Node IDs**: Use snake_case or camelCase consistently
2. **Attributes**: Use descriptive names
3. **Relationships**: Use verb phrases (`knows`, `works_at`, `reports_to`)

### Performance Tips

1. **Large graphs**: Consider breaking into multiple files with imports
2. **Attribute types**: Use appropriate data types (numbers vs strings)
3. **File size**: LGF parsing is efficient, but very large files may benefit from binary formats for production use

## Error Handling

Common parsing errors and solutions:

### Syntax Errors
```
# Error: Missing arrow
alice Person
  knows bob  # ❌ Invalid

alice Person  
  -knows-> bob  # ✅ Correct
```

### Import Errors
```python
# Handle missing files gracefully
try:
    graph = parse_lgf_file("missing.lgf")
except FileNotFoundError:
    print("LGF file not found")
```

### Invalid Data Types
```lgf
# All these are valid and will be parsed appropriately
alice Person
  age = not_a_number    # Parsed as string "not_a_number" 
  score = "42"          # Parsed as string "42"
  count = 42            # Parsed as integer 42
```

## Examples Repository

More examples can be found in the `examples/` directory:
- `social_network.lgf` - Social media relationships
- `organization.lgf` - Company hierarchy
- `knowledge_graph.lgf` - Domain knowledge representation

## See Also

- [IronWeaver API Documentation](API.md)
- [Graph Algorithms Guide](algorithms.md)
- [Performance Benchmarks](../performance_results/README.md)
