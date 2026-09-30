# Changelog

All notable changes to ironweaver (the Python package) and ironweaver-core
(the Rust crate) are listed here. Both follow [Semantic Versioning](https://semver.org/);
while the version is 0.x, a minor release (0.2 → 0.3) may break the API, and
the breaking changes are listed under their own heading.

## 0.2.0 — unreleased

The graph engine was rewritten as a pure-Rust core crate (`ironweaver-core`,
no Python dependency) with thin PyO3 bindings, and gained an analytics layer,
a query layer and database foundations.

### Breaking changes

- **File format version 2.** Saved files (JSON and binary) use format 2: a
  binary header, a checksum, labels, edge types and edge ids. Files saved by
  0.1 (format 1) still load and are migrated (`attr["labels"]` / `attr["type"]`
  become labels and types; an old edge id is kept in `meta["legacy_id"]`), but
  0.1 can't read files saved by 0.2. See [the file format docs](https://github.com/p-sodmann/Ironweaver/blob/main/docs/format.md).
- **Labels and edge types are graph fields.** `attr["labels"]` (a list of str)
  on nodes and `attr["type"]` (a str) on edges are stored as labels and types.
  Reading and writing them through `attr`, `attr_get` / `attr_set`, dict
  filters and expressions works as before.
- **Edge ids** are persistent integers assigned by the graph (`edge.id`,
  `Vertex.get_edge`), never reused.
- `Projection.label_propagation` on a directed projection follows the LDBC
  Graphalytics CDLP rule: in- and out-neighbours count separately.
- Python 3.8 is no longer supported (3.9 – 3.14 are).
- `datetime.date` / `datetime.datetime` attribute values are saved as dates
  and date-times (they used to be saved as their `str()` and load as
  strings), and `bytes` / `bytearray` as bytes (they used to become lists of
  ints). Files saved by 0.1 load as before.

### Deprecated

- `Vertex.shortest_path_bfs` and `Vertex.shortest_path_dijkstra`: use
  `shortest_path(root, target, method="bfs" | "dijkstra")`. They emit a
  `DeprecationWarning` and will be removed in a later release.

### Added

- **Analytics on `Vertex.project(...)`**, a compact read-only snapshot
  (CSR, sorted neighbour lists, optional weights, node / edge filters);
  algorithms run in Rust with the GIL released, many of them in parallel:
  - components (weak, strong), topological sort, cycle detection;
  - degree centrality, PageRank (personalized; `tol=0` for a fixed number
    of iterations), betweenness (exact or sampled), closeness, harmonic
    centrality;
  - triangles, local clustering (undirected, or LDBC's directed definition),
    k-core numbers;
  - label propagation (LDBC CDLP), Leiden communities, modularity;
  - node similarity (Jaccard, overlap, common neighbours, Adamic-Adar,
    resource allocation, preferential attachment) for pairs and top-k;
  - minimum / maximum spanning forests, k shortest paths (Yen),
    direction-optimizing BFS levels, batch shortest paths and distances;
  - FastRP embeddings and node2vec walks.
- **Pattern matching:** `Vertex.match("(a:Person)-[k:KNOWS*1..3]->(b)")`,
  Cypher-like patterns with labels, types, variable lengths, property maps,
  `where=` expressions and fixed ids. `Node.paths(...)` lists
  variable-length paths (walk / trail / path uniqueness).
- **Filter expressions evaluated in Rust:** `attr`, `label`, `edge_type`,
  combined with `&`, `|`, `~`; accepted by `filter`, `project` and `match`.
- **Labels and edge types:** `add_node(..., labels=)`, `add_edge(..., type=)`,
  `Node.labels` / `add_label` / `remove_label` / `has_label`,
  `Vertex.nodes_with_label` (indexed), `Edge.type`.
- **Bulk loading:** `Vertex.add_nodes` / `Vertex.add_edges`, all-or-nothing,
  with attributes as columns.
- **`Vertex.shortest_path`** with pluggable methods (BFS, Dijkstra, A* with
  coordinate or table heuristics), `max_cost`, `direction`.
- **Rust core (`ironweaver-core`):** `Graph<N, E>`, an op log (`Op`,
  `Graph::apply` returning the undo ops, atomic `apply_all`), `Expr`,
  `Projection` and the algorithms above, file formats, query primitives.
- **Dates, date-times and bytes** as attribute values: they keep their
  types through JSON and binary saves (aware date-times keep their UTC
  offset) and compare in filter expressions. In the core: `Value::Bytes`,
  `Value::Date`, `Value::DateTime` and `Key` (a totally ordered, hashable
  form of scalar values, for indexes).
- **Property indexes:** `Vertex.create_index(name)` / `drop_index` /
  `indexes`, and `find(name, value)` / `find_range(name, low, high)`
  (which scan when there is no index). `filter(where=...)` and `match` use
  indexes for equality, range and `is_in` conditions. Indexes follow every
  change made through the graph and are saved with it (the indexed paths,
  in `metadata.indexes`; loading rebuilds them). In the core:
  `Graph::create_index`, `find_nodes`, `find_nodes_in_range`,
  `index_candidates`, with exact results while payload changes are pending
  (`flush_indexes`).
- `Vertex.memory_usage(deep=False)` (and `Graph::memory_usage` in the
  core): bytes used by the structure, with `deep` also the attribute dicts
  and their values.
- **Ctrl+C stops long computations:** projection algorithms, batch
  shortest paths / distances, random walks, `Vertex.match`, `Node.paths`,
  `shortest_path` and traversals raise `KeyboardInterrupt` shortly after
  Ctrl+C (a signal) instead of running to the end; the graph stays usable.
  In the core, `cancel::Token` / `cancel::run` stop them from another
  thread (`GraphError::Interrupted`).
- `ironweaver.__version__`; `Path` objects now carry `edges`.
- **Core, for servers built on it** (`ironweaver-core` only):
  - visit budgets: `traversal::dfs_limited` / `bfs_limited` /
    `expand_limited`, `query::expand_paths_limited` and
    `WalkPlan::run_limited` take a `Budget` (`max_visited`, `max_results`)
    and either fail with `GraphError::BudgetExceeded` or return the first
    results with `truncated` set;
  - `Expr`, `CmpOp`, `Pattern` (and its parts) implement serde `Serialize`
    / `Deserialize`, with expression nesting capped at `MAX_EXPR_DEPTH`;
    `Pattern` implements `Display` (and `to_text`), and
    `Pattern::parse(&p.to_string())` gives back an equal pattern;
  - `LoadGraph::build_from_reader` / `format::from_binary_reader` load a
    binary file from any `Read`, building the graph while decoding (the
    file's bytes are never all in memory; the checksum is checked at the
    end and a mismatch drops the partial graph);
  - `GraphWriter::with_timestamp` fixes or omits `metadata.timestamp`, for
    byte-identical saves.
- Validation against the LDBC Graphalytics reference outputs, networkx on
  random graphs, and a benchmark against networkx, igraph, rustworkx and
  networkit (`benchmarks/compare_libraries.py`).

### Changed

- Size limits raise errors instead of panicking: more than 2^32 - 2 node
  or edge slots gives `GraphError::Capacity` (`OverflowError` in Python). A
  slot whose generation counter runs out is retired, so a handle to a
  removed node or edge can never resolve to a new one.

- Saving is atomic (temporary file, fsync, rename); loading refuses values
  nested deeper than 100 levels and checks the binary checksum.
- Saves are deterministic: attribute maps of core `Record`s (and dict
  values) are written sorted by key, in files and in serde output (op
  logs), so equal graphs with equal slot order save to equal bytes (apart
  from the save timestamp).
- `Graph::memory_usage` (and `Vertex.memory_usage()`) is O(1): the parts
  that grow with the graph are counted as it changes. Property indexes now
  count each indexed node's copy of its key.
- `Graph::apply_all` no longer panics if undoing an op fails during a
  rollback (a bug): the rest of the rollback runs and the error is
  `GraphError::Internal`. Its docs now say what a rollback doesn't restore
  (adjacency order, the edge id counter). `GraphError` is `#[non_exhaustive]`.
- In pattern text, an empty property map (`(a {})`) means no condition, and
  conditions from several mentions of a node are one flat `And`.
- Large speed-ups across traversal, pathfinding, serialization and graph
  building (see `performance_results/`).
- PyO3 0.29.

### Fixed

- A reference cycle that kept graphs alive after use.
- `attr("type")` / `attr("labels")` in expressions read the edge type and
  node labels, like the rest of the API.

## 0.1.0

First release: `Vertex` / `Node` / `Edge` graph with attribute dicts, BFS /
DFS traversal, filtering, expansion, random walks, BFS and Dijkstra shortest
paths, JSON and binary save / load, the LGF text format, callbacks, and
networkx conversion.
