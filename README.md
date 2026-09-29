# IronWeaver

![Logo](https://raw.githubusercontent.com/p-sodmann/Ironweaver/main/assets/logo.png)

A fast property graph library for Python, with its engine in Rust. Build and change graphs from Python; run traversals, shortest paths, graph analytics and pattern matching in Rust, most of them on all cores.

**Documentation:** <https://p-sodmann.github.io/Ironweaver/> · **Changelog:** [CHANGELOG.md](https://github.com/p-sodmann/Ironweaver/blob/main/CHANGELOG.md)

## Features

- **Property graph**: nodes with string ids, labels and attributes; typed edges with persistent ids; callbacks on changes; bulk loading.
- **Traversal and paths**: BFS / DFS, expansion, filtering, BFS / Dijkstra / A* shortest paths, many shortest paths at once in parallel, k shortest paths, random walks.
- **Analytics** on a read-only projection, released from the GIL and parallel:
  - components, PageRank, betweenness, closeness and harmonic centrality;
  - triangles, clustering, k-core numbers;
  - label propagation, Leiden communities, modularity;
  - node similarity, spanning trees, FastRP embeddings and node2vec walks.
- **Pattern matching**: Cypher-like patterns (`(a:Person)-[:KNOWS*1..3]->(b)`) and variable-length paths.
- **Filter expressions evaluated in Rust**: `(attr("age") > 30) & label("Person")`.
- **Checked results**: validated against networkx and the LDBC Graphalytics reference outputs, and benchmarked against networkx, igraph, rustworkx and networkit ([report](https://github.com/p-sodmann/Ironweaver/blob/main/performance_results/library_comparison.md)).
- **Files**: JSON and binary formats with checksums, atomic saves, and a [compatibility promise](https://github.com/p-sodmann/Ironweaver/blob/main/docs/format.md); the LGF text format; conversion to networkx.
- **A Rust crate**: the engine is also available on its own as [`ironweaver-core`](https://crates.io/crates/ironweaver-core).

> **Using an LLM or coding agent?** [`llms.txt`](https://github.com/p-sodmann/Ironweaver/blob/main/llms.txt) is a compact,
> fully runnable guide to the whole API (plus the gotchas) meant to be pasted
> into a model's context.

## Installation

```bash
pip install ironweaver
```

Wheels are available for Linux (x86_64, aarch64; glibc and musl), macOS (Intel and Apple Silicon) and Windows (x64), for Python 3.9–3.14. To build from source, you need a Rust toolchain (1.85 or newer):

```bash
pip install maturin
pip install -e .
```

## Quick Start

### Basic Graph Creation

```python
from ironweaver import Vertex, Node, Edge
import networkx as nx
import matplotlib.pyplot as plt

# Create a new graph
graph = Vertex()

# Add nodes with attributes
node1 = graph.add_node('node1', {'value': 1, 'type': 'start'})
node2 = graph.add_node('node2', {'value': 2, 'type': 'process'})
node3 = graph.add_node('node3', {'value': 3, 'type': 'end'})

# Add edges with weights
edge1 = graph.add_edge('node1', 'node2', {'weight': 1.0})
edge2 = graph.add_edge('node2', 'node3', {'weight': 2.0})
edge3 = graph.add_edge('node1', 'node3', {'weight': 3.0})

print(f"Graph: {graph}")
print(f"Node count: {graph.node_count()}")
```

### Graph Visualization

```python
# Convert to NetworkX for visualization
nx_graph = graph.to_networkx()

# Visualize with matplotlib
plt.figure(figsize=(10, 8))
pos = nx.spring_layout(nx_graph)
nx.draw(nx_graph, 
        pos=pos,
        with_labels=True, 
        node_color='lightblue', 
        node_size=1000,
        font_size=12,
        font_weight='bold', 
        edge_color='gray',
        arrows=True)
plt.title("Graph Visualization")
plt.show()
```

## Advanced Features

### Filtering with Lambdas

Filter nodes using expressive lambda predicates. The argument `n` is a [`NodeView`](https://github.com/p-sodmann/Ironweaver/blob/main/docs/filtering.md) — a read-only proxy exposing `.id`, `.type`, `.attr(key)`, `.degree`, `.has_edge_to(id)`, and more:

```python
# Keep active nodes of certain types with high scores
result = graph.filter(lambda n: (
    n.id.startswith("test_")
    and n.type in {"A", "B", "C"}
    and n.attr("score") < 0.8
    and n.attr("status") != "archived"
))
```

Also supports ID-based and attribute-based filtering:

```python
sub = graph.filter(ids=["node1", "node2"])
sub = graph.filter(id="node1")                        # single node
sub = graph.filter(type="process")                    # attribute equality
sub = graph.filter(type="process", status="active")   # multiple kwargs are ANDed
```

> **Note:** calling `graph.filter()` with no arguments raises `ValueError`. Exactly one filtering mode must be used. Mixing modes (e.g. a predicate *and* keyword args) also raises `ValueError`.

See the [Filtering Documentation](https://github.com/p-sodmann/Ironweaver/blob/main/docs/filtering.md) for the full `NodeView` API.

### Shortest Path Finding

```python
# Returns a Vertex subgraph containing only the path nodes.
# Raises ValueError if either node is missing or the target is unreachable.
result = graph.shortest_path('node1', 'node3', method='bfs')
print(f"Path order: {result.meta['nodelist']}")  # ordered node IDs
print(f"Path nodes: {result.keys()}")

# Visualize the path
path_nx = result.to_networkx()
nx.draw(path_nx, with_labels=True, node_color='orange')
plt.title("Shortest Path from node1 to node3")
plt.show()

# Ignore edge direction, or walk edges backwards
result = graph.shortest_path('node3', 'node1', method='bfs', direction='both')

# Weighted shortest path (Dijkstra). Costs come from the "weight" edge
# attribute; edges without it cost default_weight (1.0).
result = graph.shortest_path('node1', 'node3', method='dijkstra', weight='weight')
print(result.meta['nodelist'], result.meta['cost'])

# One entry point for every algorithm: method="bfs", "dijkstra" or "astar"
result = graph.shortest_path('node1', 'node3', method='dijkstra')
print(graph.path_methods())

# A* uses node coordinates (or precomputed estimates) to search toward the target
graph.get_node('node1').attr_set('pos', (0.0, 0.0))
graph.get_node('node3').attr_set('pos', (1.0, 0.0))
result = graph.shortest_path('node1', 'node3', method='astar', coords='pos')
```

### Graph Expansion

```python
# Create a subgraph and expand it by exploring neighbors
filtered_graph = graph.filter(ids=['node2'])  # Start with just node2
expanded = filtered_graph.expand(graph, depth=1)  # Expand 1 level

print(f"Original nodes: {graph.keys()}")
print(f"Filtered nodes: {filtered_graph.keys()}")
print(f"Expanded nodes: {expanded.keys()}")
```

> **Note:** by default `expand` follows **outgoing** edges only, so nodes that have edges *pointing into* the seed nodes are not pulled in. Pass `direction="in"` to follow incoming edges, or `direction="both"` for both.


### BFS / DFS Traversal

`Node.bfs()` and `Node.traverse()` return a `Vertex` subgraph. The visit order is in `result.meta["nodelist"]`:

```python
# BFS from a node — all reachable nodes up to depth 2
reachable = graph["node1"].bfs(depth=2)
print(reachable.meta["nodelist"])   # BFS discovery order: ['node1', 'node2', 'node3']
print(reachable.keys())             # same nodes, dict order

# Filter which edges to follow — the callable receives an EdgeView
light_edges = graph["node1"].bfs(filter=lambda e: e.attr("weight") < 2.5)

# DFS variant; also accepts a dict shorthand for edge attribute matching
dfs_result = graph["node1"].traverse(depth=3, filter={"weight": 1.0})
```

### BFS Search

`bfs_search` finds a single target node and returns it (or `None` if unreachable), without building a subgraph:

```python
# Returns the Node object if found, None otherwise
target = graph["node1"].bfs_search("node3", depth=3)
if target is not None:
    print(f"Found: {target.id}, attrs: {target.attr}")
else:
    print("Not reachable within depth 3")
```

Use `bfs_search` when you only need to know *whether* a node is reachable and want the `Node` itself; use `bfs` when you need the full reachable subgraph.


### Random Walks

```python
# Only the first three arguments are required
walks = graph.random_walks("node1", 5, 20)   # start node, max hops, attempts

# Optional keyword arguments tune the walks
walks = graph.random_walks(
    "node1", 5, 20,
    min_length=2,             # minimum walk length to keep (default: no minimum)
    allow_revisit=False,      # allow visiting the same node twice (default: False)
    include_edge_types=True,  # interleave edge-type strings: ["a", "knows", "b", ...]
    edge_type_field="type",   # attribute key for the edge type (default: "type")
)
for walk in walks:
    print(walk)
```

Duplicate walks are removed automatically.

**Stratified mode** aims for equal node visit frequencies: every choice is
weighted by `1 / (1 + times_visited)`, steering walks towards the
least-visited nodes. Visit counts persist across all attempts of one call.
With `stratified=True`, `start_node_id` may be `None` — each walk's start is
then sampled across the whole graph with the same inverse-count weighting:

```python
# Evenly explore the whole graph (start nodes sampled, steps biased
# towards least-visited nodes)
walks = graph.random_walks(None, 5, 50, stratified=True)

# Fixed start, but steps still favour least-visited nodes
walks = graph.random_walks("node1", 5, 50, stratified=True)
```

Pass `seed=` to get reproducible walks. Walks run in native code with the GIL
released, and non-stratified walks are spread across all CPU cores:

```python
walks = graph.random_walks("node1", 10, 100_000, allow_revisit=True, seed=42)
```

### Event-Driven Programming

```python
# Set up callbacks for graph modifications
def on_node_added(vertex, node):
    print(f"Node added: {node.id}")
    # Track visited nodes in metadata
    if "visited_nodes" not in vertex.meta:
        vertex.meta["visited_nodes"] = []
    vertex.meta["visited_nodes"].append(node.id)
    return True  # Return False to stop subsequent callbacks in this chain

def on_edge_added(vertex, edge):
    print(f"Edge added: {edge.from_node.id} -> {edge.to_node.id}")
    edge.meta["timestamp"] = "2025-05-25"
    return True

# Register callbacks
graph.on_node_add_callbacks.append(on_node_added)
graph.on_edge_add_callbacks.append(on_edge_added)

# Add nodes and edges - callbacks will be triggered
new_node = graph.add_node('node4', {'value': 4})
new_edge = graph.add_edge('node3', 'node4', {'weight': 1.5})

print(f"Graph metadata: {graph.meta}")
```

> **Note:** returning `False` from a callback stops subsequent callbacks in that chain, but the node or edge is **always added** regardless.

### Persistence

```python
import json

# Save to a file
graph.save_to_json("my_graph.json")

# Save to a string (omit the path — returns the JSON string instead of writing a file)
json_str = graph.save_to_json()

# Output is compact by default; pass pretty=True for indented, human-readable JSON
graph.save_to_json("my_graph.json", pretty=True)

# Save to binary format (more efficient for large graphs)
graph.save_to_binary("my_graph.bin")
# Or use half-precision floats to reduce file size
graph.save_to_binary_f16("my_graph_f16.bin")

# Load from a file path, a raw JSON string, or a plain dict
loaded_graph = Vertex.load_from_json("my_graph.json")   # file path
loaded_graph = Vertex.load_from_json(json_str)           # JSON string
loaded_graph = Vertex.load_from_json(json.loads(json_str))  # plain dict
print(f"Loaded graph: {loaded_graph}")
print(f"Metadata: {loaded_graph.get_metadata()}")
```

Saving to a file is atomic: the graph is written to a temporary file next to the target and renamed over it, so a failed or interrupted save never leaves a half-written file (the previous file stays as it was). Attribute values may nest lists and dicts at most 100 levels deep; saving deeper values (or a list that contains itself) and loading files with deeper values raise `RuntimeError`.

### LGF (Labeled Graph Format) Support

IronWeaver supports reading graphs from the Labeled Graph Format (LGF), which provides a human-readable text format for representing graphs with nodes, edges, and attributes.

```python
from ironweaver import parse_lgf, parse_lgf_file

# Parse LGF from string
lgf_content = """
person_1 Person
  name = "Alice"
  age = 30
  -knows-> person_2
    since = 2020
  -works_at-> company_1

person_2 Person
  name = "Bob"
  age = 25

company_1 Company
  name = "Tech Corp"
  <-founded_by- person_1
"""

graph = parse_lgf(lgf_content)
print(f"Parsed {graph.node_count()} nodes")

# Parse LGF from file
graph = parse_lgf_file("my_graph.lgf")
```

#### LGF Syntax

**Node Declaration:**
```
node_id NodeType
  attribute = "value"
  number_attr = 42
  boolean_attr = true
```

> **Note:** the `NodeType` label is stored in `attr["labels"]` as a **list**
> (e.g. `{"labels": ["NodeType"]}`), not in `attr["type"]`. A node declared
> multiple times has its labels merged into that list. This means
> `graph.filter(type="NodeType")` and `NodeView.type` do **not** match
> LGF-parsed nodes — filter on the label list instead:
>
> ```python
> from ironweaver.filter.predicates import attr_contains
> clinicians = graph.filter(attr_contains("labels", "NodeType"))
> # or: graph.filter(lambda n: "NodeType" in n.attr("labels", []))
> ```

**Edge Declaration (New Syntax):**
```
# Forward relationship: from current node to target
  -relationship_type-> target_node

# Inverse relationship: from target node to current node  
  <-relationship_type- target_node
```

**Supported Data Types:**
- **Strings**: `"quoted text"` or `'quoted text'` 
- **Numbers**: `42`, `3.14`
- **Booleans**: `true`, `false`
- **Unquoted values**: treated as strings

**Import Support:**
```
import("other_file.lgf")
```

See the [LGF Documentation](https://github.com/p-sodmann/Ironweaver/blob/main/docs/LGF.md) for detailed syntax and examples.

## Gotchas

These behaviours surprise people (and LLMs) most often:

- **`node.attr`, `node.meta`, `node.edges`, `node.inverse_edges`, `edge.attr`,
  `edge.meta` and `vertex.nodes` return copies.** Mutating the returned
  object has no effect: `node.attr["x"] = 1` is silently lost, and so is
  `vertex.nodes["id"] = node`. Use `node.attr_set("x", 1)` /
  `edge.attr_set(...)`, assign a whole dict (`node.attr = {...}`), and add or
  remove nodes and edges with `add_node` / `add_edge` / `remove_node` /
  `remove_edge`.
- **`vertex.meta` and the `on_*_callbacks` lists are live** Python objects:
  `vertex.meta["k"] = v` and `vertex.on_node_add_callbacks.append(cb)` work.
- **Result graphs.** `node.bfs()`, `node.traverse()`, `filter`, `expand`
  and `shortest_path` return new graphs with *copies* of the nodes and edges, so
  changing them does not touch the source graph (a `filter` result does
  share the source's `meta` dict and callback lists).
- **Nodes and edges are handles.** The graph data lives in Rust; a `Node` or
  `Edge` object refers to one node or edge of its `vertex`. Compare them with
  `==` (`graph["a"] == graph["a"]`), not `is`. Edges are only created with
  `add_edge` (`Edge(...)` raises `TypeError`); `Node(id, attr)` makes a node
  in its own one-node `Vertex`. `remove_node` returns a detached copy, and
  using an old handle to a removed node or edge raises `RuntimeError`.
- **Don't change the graph from a traversal filter.** While `bfs` /
  `traverse` / `bfs_search` run, a callable `filter` may read the graph, but
  changing it (adding or removing nodes and edges, assigning attributes) can
  raise `RuntimeError`.
- **Ordered results live in `meta`.** Traversal order and paths are in
  `result.meta["nodelist"]`; `result.keys()` is graph order (insertion order
  until nodes are removed), not visit order.
- **Shortest-path ties.** When several shortest paths exist,
  `shortest_path` returns one of them; which one is not specified.
- **`Vertex.load_from_json(text)`** treats a string starting with `{` as JSON
  and any other string as a file path.

## API Reference

### Core Classes

#### `Vertex`
The main graph container that holds nodes and provides graph-level operations.

```text
# Create empty graph
graph = Vertex()

# Node operations
node = graph.add_node(id: str, attr: dict = None) -> Node
node = graph.get_node(id: str) -> Node
exists = graph.has_node(id: str) -> bool
exists = "node1" in graph        # membership test, same as has_node
count = graph.node_count() -> int

# Edge operations  
edge = graph.add_edge(from_id: str, to_id: str, attr: dict = None) -> Edge

# Removal (neighbours' edge lists are kept consistent)
node = graph.remove_node(id: str) -> Node                           # also drops its edges; returns a detached copy
count = graph.remove_edge(from_id: str, to_id: str, attr: dict = None) -> int

# Algorithms
result = graph.shortest_path(source: str, target: str, method: str = None,
                             *, weight: str = None, default_weight: float = 1.0,
                             max_cost: float = None, direction: str = "out",
                             **options) -> Vertex
# method: "bfs" (option max_depth), "dijkstra", or "astar" (options heuristic=
# "euclidean"|"manhattan" with coords=["x", "y"] | "pos" | ["pos.lat", "pos.lon"],
# or distances="<vertex.meta key>"). None: astar if an A* option is given,
# dijkstra if weight is given, else bfs. meta: nodelist, cost, method, expanded.
methods = Vertex.path_methods() -> dict   # {name: description}
# result.meta["nodelist"] contains the ordered path; raises ValueError if unreachable
# shortest_path_bfs / shortest_path_dijkstra are deprecated: use method="bfs" / "dijkstra"
paths = graph.shortest_paths(pairs: list[tuple[str, str]], method=None, *, weight=None,
                             default_weight=None, max_cost=None, direction="out")
# -> [{"nodelist": [...], "cost": ...} or None, ...]; many queries in parallel, GIL released
dist = graph.distances(sources: list[str], targets: list[str] = None, method=None, ...)
# -> {source: {node: cost}}; "bfs" or "dijkstra" only
proj = graph.project(weight=None, default_weight=None, *, direction="out", nodes=None,
                     node_filter=None, edge_filter=None) -> Projection
# compact read-only snapshot for analytics: proj.shortest_paths(pairs, method=None, *, max_cost=None),
# proj.distances(...), proj.neighbors(id, "out"|"in"), proj.degree(id), proj.ids(), proj.memory_usage()
# analytics (docs/analytics.md): proj.weakly_connected_components(), proj.strongly_connected_components(),
# proj.topological_sort(), proj.find_cycle(), proj.degree_centrality("out"|"in"),
# proj.pagerank(alpha=0.85, *, personalization=None, max_iter=100, tol=1e-6), proj.triangles(),
# proj.clustering(), proj.core_number(), proj.label_propagation(max_iter=20),
# proj.bfs_levels(sources, max_depth=None), proj.betweenness_centrality(k=None, ...),
# proj.closeness_centrality(), proj.harmonic_centrality(), proj.similarity(pairs, metric="jaccard"),
# proj.most_similar(ids=None, k=10), proj.leiden(resolution=1.0, *, seed=None), proj.modularity(communities),
# proj.minimum_spanning_tree(), proj.k_shortest_paths(source, target, k), proj.fastrp(dimension=128),
# proj.node2vec_walks(walk_length=80, walks_per_node=10, *, p=1.0, q=1.0)
node = graph.add_node(id, attr=None, labels=None)       # node.labels, add_label, remove_label, has_label
graph.add_nodes(ids_or_id_attr_pairs, *, labels=None, attrs=None) -> int     # bulk; attrs as columns
graph.add_edges(pairs_or_triples, *, type=None, attrs={"weight": [...]}) -> int   # bulk; all-or-nothing
edge = graph.add_edge(from_id, to_id, attr=None, type=None)   # edge.type, edge.id (persistent int)
graph.get_edge(edge_id) -> Edge; graph.nodes_with_label(label) -> list[Node]
graph.create_index(name) -> bool; graph.drop_index(name); graph.indexes -> list[str]
graph.find(name, value) -> list[Node]; graph.find_range(name, low=None, high=None, *, inclusive="both")
# expressions evaluated in Rust: from ironweaver import attr, label, edge_type
graph.filter((attr("age") > 30) & label("Person")); graph.project(edge_filter=edge_type("knows"))
rows = graph.match("(a:Person)-[k:knows*1..2]->(b)", where=None, ids=None, limit=None)
# -> [{"a": Node, "k": [Edge, ...], "b": Node}, ...]; Cypher-like patterns (docs/patterns.md)
paths = node.paths(min_hops=1, max_hops=None, *, direction="out", types=None, where=None,
                   uniqueness="trail", limit=None) -> list[Path]   # path.nodes, path.edges
expanded = graph.expand(source: Vertex, depth: int = 1, direction: str = "out") -> Vertex
filtered = graph.filter(predicate) -> Vertex   # lambda/callable — raises ValueError if no args
filtered = graph.filter(**filters) -> Vertex    # id, ids, or attribute=value filters
pruned_count = graph.prune() -> int            # always 0: edges never dangle (kept for compatibility)
walks = graph.random_walks(start_node_id, max_length, num_attempts,
                            min_length=None, allow_revisit=False,
                            include_edge_types=False,
                            edge_type_field="type",
                            stratified=False, seed=None) -> list[list[str]]
# stratified=True biases every choice towards least-visited nodes;
# start_node_id may then be None to sample starts across the whole graph

# Conversion and analysis
nx_graph = graph.to_networkx() -> networkx.DiGraph
metadata = graph.get_metadata() -> dict

# Persistence
graph.save_to_json("path.json")              # write to file
json_str = graph.save_to_json()              # no arg → returns JSON string
json_str = graph.save_to_json(pretty=True)   # indented (default: compact)
graph.save_to_binary(file_path: str)
graph.save_to_binary_f16(file_path: str)
loaded = Vertex.load_from_json(source)       # file path, JSON string, or dict
loaded = Vertex.load_from_binary(file_path: str) -> Vertex
```

#### `Node`
Represents individual vertices in the graph with attributes and edges.

```text
# Access node properties (attr, meta and edges return copies, see Gotchas)
id = node.id        # Node identifier
attrs = node.attr   # Node attributes dict (a copy)
edges = node.edges  # List of outgoing edges (a copy)

# Traversal — result.meta["nodelist"] contains visit order
reachable = node.traverse(depth: int = None) -> Vertex   # DFS
bfs_result = node.bfs(depth: int = None) -> Vertex       # BFS
# Both accept filter= (dict for attr match, or callable receiving EdgeView)

# Search: returns the Node if found, None otherwise
found = node.bfs_search(target_id: str, depth: int = None) -> Node | None

# Changing attributes: node.attr[key] = value is silently lost (attr is a copy)
node.attr_set(key, value)   # sets one key and fires on_update_callbacks
node.attr = {...}           # replaces the whole dict (no callbacks)

# Append to a list attribute (creates the list if the key is missing)
node.attr_list_append("tags", "urgent")
node.attr_list_append("tags", "reviewed")   # node.attr["tags"] == ["urgent", "reviewed"]
```

#### `Edge`
Represents connections between nodes with optional attributes.

```text
# Access edge properties
from_node = edge.from_node  # Source node
to_node = edge.to_node      # Target node
attrs = edge.attr           # Edge attributes dict (a copy; use edge.attr_set)
```

#### `Path`

`Node.paths(...)` returns `Path` objects: `path.nodes`, `path.edges`, `path.ids()`, and `len(path)` (the number of edges). `shortest_path` and the traversals return a `Vertex` subgraph instead; its `meta["nodelist"]` holds the ordered node ids.

## Using the Rust core directly

The graph and all algorithms live in a pure-Rust crate,
[`crates/ironweaver-core`](https://github.com/p-sodmann/Ironweaver/tree/main/crates/ironweaver-core), which has no Python
dependency; the Python module is a thin PyO3 layer on top of it. Rust code can
use the core on its own:

```toml
[dependencies]
ironweaver-core = { path = "crates/ironweaver-core" }
```

```rust
use ironweaver_core::pathfinding::{find_path, PathQuery};
use ironweaver_core::{Graph, GraphError, Record, Value};

let mut g: Graph<Record, Record> = Graph::new();
let a = g.add_node("a", Record::default())?;
let b = g.add_node("b", Record::default())?;
g.add_edge(a, b, Record::with_attr([("weight", Value::from(2.0))]))?;
let path = find_path::<_, _, GraphError>(&g, a, b, &mut PathQuery::dijkstra())?.unwrap();
assert_eq!(path.cost, 2.0);
```

`Graph<N, E>` is generic over the node and edge payloads; algorithms read
them through the `Attributes` trait (implemented by `Record`, a map of
`Value`s). `ironweaver_core::format` reads and writes the same JSON / binary
files as the Python `save_to_*` / `load_from_*` methods.

## Performance

`IronWeaver` is built with performance in mind:

- **Rust Backend**: Core algorithms implemented in Rust for maximum speed
- **Memory Efficient**: Optimized data structures for large graphs
- **Minimal Overhead**: nodes and edges are stored in Rust (no Python object per node or edge); PyO3 bindings provide near-native performance
- **Scalable**: Tested with graphs containing thousands of nodes and edges

### Benchmarks

The library includes comprehensive performance testing for various graph operations:
- BFS traversal performance
- Shortest path algorithms
- Graph expansion operations
- Memory usage optimization
- Cyclic graph handling

See the `performance_results/` directory for detailed benchmarks.

To compare against networkx on your machine (writes
`performance_results/networkx_comparison.md` with a timing table):

```bash
pip install networkx
python benchmarks/compare_networkx.py                      # default sizes
python benchmarks/compare_networkx.py --sizes 50000:250000 --repeats 5
```

For memory use (resident graph size, peak RSS while loading and saving JSON,
file sizes; Linux or `psutil`), `benchmarks/compare_networkx_memory.py` writes
`performance_results/networkx_memory.md`:

```bash
python benchmarks/compare_networkx_memory.py               # 10k and 100k nodes
python benchmarks/compare_networkx_memory.py --sizes 20000:100000 --repeats 5
```

## Contributing

Contributions are welcome! Please see our contributing guidelines for more information.

### Development Setup

```bash
# Clone the repository
git clone <repository-url>
cd ironweaver

# Build the Rust extension
maturin develop

# Run tests
pytest                        # Python API (needs the built extension)
cargo test -p ironweaver-core # pure-Rust core
```

## License
Licensed under the MIT License. See the LICENSE file for details.

## Acknowledgments

- Built with [PyO3](https://pyo3.rs/) for Rust-Python bindings
- Compatible with [NetworkX](https://networkx.org/) for visualization
- Inspired by the need for high-performance graph operations in Python

---
**Note**: This library is designed for applications requiring high-performance graph operations. For simple use cases, NetworkX might be more appropriate. For complex, large-scale graph analysis, `IronWeaver` might provide significant performance advantages.
Initially it was developed for the [Gustabor](https://gustabor.de) project.

