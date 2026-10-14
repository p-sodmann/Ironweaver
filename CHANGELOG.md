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

- **Binary files saved by 0.1 are no longer supported.** Loading a format 1
  binary file (`load_from_binary`, `format::from_binary`) raises an error
  saying it is format 1. The reader for it was the only user of bincode 1.x
  (unmaintained, RUSTSEC-2025-0141), which is no longer a dependency, and
  `ironweaver-core` has no `format-v1` feature. To convert such a file, load
  it with ironweaver 0.1 and save it with `save_to_json`, then load the JSON
  file (see [the file format docs](https://github.com/p-sodmann/Ironweaver/blob/main/docs/format.md#format-1-ironweaver-01)).
- **File format version 2.** Saved files (JSON and binary) use format 2: a
  binary header, a checksum, labels, edge types and edge ids. JSON files saved
  by 0.1 (format 1) still load and are migrated (`attr["labels"]` / `attr["type"]`
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
  ints). JSON files saved by 0.1 load as before.

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
- **`Projection.from_arrays(ids, src, dst, weight=None, coords=None, *,
  direction=None)`** builds a projection from node ids and edge arrays
  (numpy int64 / int32 and float64 / float32 through the buffer protocol,
  or sequences) without a Vertex, with the same results as projecting a
  Vertex with those edges in that order. On a 180k-node / 700k-edge grid it
  takes about 60 ms instead of about 1.8 s through `add_nodes` /
  `add_edges` / `project`. In the core: `Projection::from_edges` and
  `RawProjection::with_coords` (node coordinates, for A*). (#114)
- **`Projection.set_weights(edges, weights)`** changes edge weights in place;
  the structure stays. Edges are numbered in projected order: the order
  given to `from_arrays`, or the Vertex's edge order among the projected
  edges (`Projection.edge_index(edge)`). Every entry an edge became is
  updated (both ends on a `"both"` projection, the reversed adjacency if
  built), so every method gives the same results as a projection rebuilt
  with the new weights, at a cost proportional to the number of edges
  changed: about a microsecond for the 4 edges into a tile of a
  180,000-tile grid, against about 60 ms for a rebuild
  (`benchmarks/compare_set_weights.py`). An infinite weight makes an edge
  impassable. `set_weights` raises `RuntimeError` while a query runs on the
  projection in another thread. Weighted projections keep 4 bytes per
  adjacency entry for this (plus 4 per entry of the reversed adjacency once
  built, and 8 per edge for weighted Vertex projections). In the core:
  `Projection::set_weights`, `Projection::edge_index`. (#115)
- **A* on projections:** `Projection.shortest_paths(pairs, "astar",
  heuristic="euclidean"|"manhattan", heuristic_scale=1.0)` reads node
  coordinates compiled into the projection, by
  `Vertex.project(coords=["x", "y"] | "pos")` or
  `Projection.from_arrays(..., coords=...)`. With an admissible estimate
  (no edge cheaper than `heuristic_scale` times the distance between its
  ends) the costs equal Dijkstra's; nodes reached again more cheaply are
  re-expanded, so the estimate need not be consistent. On a 180k-tile grid
  (`benchmarks/compare_astar.py`) it is about 4-5x faster per trip than
  projection Dijkstra on one thread. `Vertex.shortest_paths` takes
  `"astar"` too (with `coords`, `heuristic`, `heuristic_scale`);
  `distances` refuses it (one source to every node has no single target).
  In the core: `batch::shortest_paths_astar`, `batch::AStar`,
  `RawProjection::with_graph_coords`. (#113)
- **Pattern matching:** `Vertex.match("(a:Person)-[k:KNOWS*1..3]->(b)")`,
  Cypher-like patterns with labels, types, variable lengths, property maps,
  `where=` expressions and fixed ids. `Node.paths(...)` lists
  variable-length paths (walk / trail / path uniqueness).
- **Filter expressions evaluated in Rust:** `attr`, `label`, `edge_type`,
  combined with `&`, `|`, `~`; accepted by `filter`, `project` and `match`.
- **Labels and edge types:** `add_node(..., labels=)`, `add_edge(..., type=)`,
  `Node.labels` / `add_label` / `remove_label` / `has_label`,
  `Vertex.nodes_with_label` (indexed), `Edge.type`. `Vertex.labels()`
  lists every label with its node count, and `Vertex.edge_types()` /
  `edge_type_count(type)` give the number of edges per type, from counts
  the graph keeps (core: `Graph::labels`, `edge_types`, `edge_type_count`).
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
  (`flush_indexes`). An index can be built off the graph:
  `Graph::begin_index_build` (O(1)) returns an `IndexBuild` that reads keys
  through `&Graph` (under a read lock, in chunks), and `install_index` swaps
  it in, in time proportional to the nodes changed meanwhile, which are
  marked dirty.
- `Vertex.memory_usage(deep=False)` (and `Graph::memory_usage` in the
  core): bytes used by the structure, with `deep` also the attribute dicts
  and their values.
- Payload memory in the core: payloads that implement the new `HeapSize`
  trait (`Record`, `Value`, `()`) are counted in `Graph::memory_usage` once
  `Graph::count_payloads()` turns it on (the `format` loaders do). It stays
  O(1): the graph keeps the count as nodes, edges and payloads change, and
  measures payloads handed out by `node_mut` / `nodes_mut` again until the
  next change. `IndexBuild::memory_usage()` (O(1)) and
  `RawProjection::memory_usage()` report what an index build in progress
  and a collected projection hold.
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
    `WalkPlan::run_limited` take a `Budget` (`max_visited`, `max_edges`,
    `max_results`) and either fail with `GraphError::BudgetExceeded` or
    return the first results with `truncated` set. `max_edges` counts every
    edge examined, so a node with millions of edges can't run past the
    budget, and the traversals check for cancellation once per edge.
    `dfs_limited` / `bfs_limited` take a `Direction` (incoming edges, or
    both ways), and `expand_limited` an edge filter.
    Shortest paths (`pathfinding::find_path_limited`, and
    `traversal::bidirectional_bfs_limited`), pattern matching
    (`query::for_each_match_limited` / `find_matches_limited`) and walk
    planning (`random_walks::plan_limited`) take one too, and poll
    cancellation per edge. The matcher streams the paths of a
    variable-length edge instead of collecting them first, and walks from
    a start node index only the part of the graph they can reach;
  - `Expr`, `CmpOp`, `Pattern` (and its parts) implement serde `Serialize`
    / `Deserialize`, with expression nesting capped at `MAX_EXPR_DEPTH`;
    `Pattern` implements `Display` (and `to_text`), and
    `Pattern::parse(&p.to_string())` gives back an equal pattern;
  - `LoadGraph::build_from_reader` / `format::from_binary_reader` load a
    binary file from any `Read`, building the graph while decoding (the
    file's bytes are never all in memory; the checksum is checked at the
    end and a mismatch drops the partial graph);
  - `GraphWriter::with_timestamp` fixes or omits `metadata.timestamp`, for
    byte-identical saves;
  - `record::lookup` reads an attribute path from an `Attrs` map with the
    same rules as `Record`, for other payload types that store `Attrs`;
  - progress of long algorithms: run one with
    `cancel::run_with_progress(&token, &progress, || ...)` and read
    `progress.snapshot()` (phase, done, total) from another thread. The
    `algo` functions (PageRank and label propagation per iteration, Leiden
    per run, triangles, core number, components, closeness, betweenness,
    similarity and node2vec per node, source or walk) and the batch
    queries report it. Core number and the connected components now
    check for cancellation too;
- Validation against the LDBC Graphalytics reference outputs, networkx on
  random graphs, and a benchmark against networkx, igraph, rustworkx and
  networkit (`benchmarks/compare_libraries.py`).
- `Graph::index_stats(path)` / `Vertex.index_stats(name)`: an index's
  entry count, distinct keys, memory and dirty node count, in O(1).
- `Graph::index_plan(expr)` (core): how `index_candidates` would find a
  filter's candidates (`IndexPlan`: a label, a point, `In` or range lookup
  on an index, a union, or nothing), without reading postings, with
  `index_plan_estimate` for its size and `execute_index_plan` to run it.
  `index_candidates` is the two in a row, so an `explain` can't disagree
  with it. In an `And`, the part with the smallest estimate is now used
  (it used to look up every part and keep the smallest result).

### Changed

- **Equal-cost paths are chosen by a stated rule** (#116): walking back
  from the target, each step goes to the earliest neighbour in node order
  (insertion order on a Vertex, the index on a Projection) through which a
  cheapest route arrives. `Vertex.shortest_path` (`"bfs"`, `"dijkstra"`),
  `Vertex.shortest_paths`, `Projection.shortest_paths` and the first path
  of `k_shortest_paths` all follow it, so they agree, and the result no
  longer depends on the order edges were added, on heap push order or on
  the number of threads (it was deterministic before, but decided by edge
  order and heap internals, and the bidirectional `"bfs"` by where its
  frontiers met). `"astar"` applies the rule to the nodes it expands
  (open list ordered by f, then larger g, then node order). Over a
  zero-weight edge a route replaces the first one found only if strictly
  cheaper. Cost: on a grid full of ties, about 9% more instructions for
  batch `"bfs"`, 6% for batch `"dijkstra"` and none for batch `"astar"`;
  for `Vertex.shortest_path`, 4% for `"dijkstra"`, 2% for `"astar"` and
  20-30% for the bidirectional `"bfs"` search, which now marks the
  target's side of the shortest paths to pick the canonical one (a few
  percent of the whole call). Documented in docs/traversal.md ("Equal-cost
  paths"); in the core: `pathfinding::best_first`, `batch`,
  `traversal::bidirectional_bfs`, `algo::k_shortest_paths`.
- Infinite edge weights on a projection mean "impassable" everywhere
  (#115): hop-count searches on a weighted projection (`"bfs"` shortest
  paths and distances, `bfs_levels`, unweighted betweenness / closeness /
  harmonic centrality and k shortest paths) skip such edges, like the
  weighted ones already did; node2vec walks never take them; spanning
  trees leave them out; PageRank, FastRP, Leiden and modularity count them
  as weight 0 (they gave NaN before).

- The minimum supported Rust version is 1.99; both crates use edition 2024.
  `rand` is 0.9 (was 0.8): a seed still gives the same result on every run,
  whatever the number of cores, but random walks, node2vec walks and Leiden
  give different results for a given seed than builds with `rand` 0.8
  (FastRP and sampled betweenness don't change).

- Size limits raise errors instead of panicking: more than 2^32 - 2 node
  or edge slots gives `GraphError::Capacity` (`OverflowError` in Python). A
  slot whose generation counter runs out is retired, so a handle to a
  removed node or edge can never resolve to a new one.

- Saving is atomic (temporary file, fsync, rename); loading refuses values
  nested deeper than 100 levels and checks the binary checksum. On Unix and
  Windows the directory is fsynced after the rename, and
  `format::write_atomic` returns an error if that fails (it was ignored), so
  `Ok` means the rename is durable within the filesystem's guarantees.
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
  `Op::RemoveNode`, `Op::RenameNode` and `Op::RemoveEdge` no longer panic
  either if the graph's id index or adjacency lists are inconsistent: `apply`
  checks before changing anything and returns `GraphError::Internal`.
- In pattern text, an empty property map (`(a {})`) means no condition, and
  conditions from several mentions of a node are one flat `And`.
- Large speed-ups across traversal, pathfinding, serialization and graph
  building (see `performance_results/`).
- PyO3 0.29.

### Fixed

- Weighted betweenness centrality with zero-weight edges no longer gives
  NaN: a node's predecessors are the neighbours settled before it on a
  shortest path, also at the same distance.
- `Expr` serde keeps its depth-limit message ("expression nested more than
  100 levels deep") under the binary encoding: postcard drops custom
  messages, and `format::take_error` (now public) returns it after a failure.
- Binary files with a non-zero `flags` or `reserved` header field are
  refused (they loaded as if intact): an unknown flag as "not supported
  (written by a newer ironweaver?)", a non-zero reserved field as damage.
  Both loaders check it; files written by ironweaver always have zeros there.
- A reference cycle that kept graphs alive after use.
- `attr("type")` / `attr("labels")` in expressions read the edge type and
  node labels, like the rest of the API.
- JSON files keep `-0.0` (it loaded as `0.0`), and NaN and ±infinity
  attribute values are saved as `"NaN"`, `"Infinity"` and `"-Infinity"`
  (they were written as `null`, and the file then failed to load).
  `Value`'s own serde does the same in JSON (and accepts those strings), so
  custom codecs that write attributes with `value::serialize_sorted`, and
  JSON `Op`s and `Value`s, keep them too.
- `Value`'s serde counts depth like the file format (a value is depth 1,
  a container's items one deeper), so an empty list or dict at depth 100
  that a file can hold can also be encoded with serde (and in `Op`s); it
  was rejected one level early.
- A JSON graph file with a deeply nested unknown field (thousands of
  levels) crashed the process with a stack overflow, in Python too: the JSON
  parser skips unknown fields recursively. `format::from_json` and
  `load_from_json` now refuse documents nested more than 255 levels deep
  (the deepest valid file needs 204).
- The serde form of `Value`, `Expr` and `Pattern` can be read from JSON up
  to the documented depth limit: `Value::from_json_str`,
  `Expr::from_json_str` and `Pattern::from_json_str` lift the JSON parser's
  own recursion limit (`serde_json` stops at 64 levels of `List` / `Dict` /
  `And` / `Or`) and rely on the core's depth counters. `Expr`'s struct
  variants, `NodePattern`, `EdgePattern`, `Pattern` and `Hops` refuse
  unknown fields instead of skipping them (skipping recursed past the
  depth counters, so a crafted filter could overflow the stack).

## 0.1.0

First release: `Vertex` / `Node` / `Edge` graph with attribute dicts, BFS /
DFS traversal, filtering, expansion, random walks, BFS and Dijkstra shortest
paths, JSON and binary save / load, the LGF text format, callbacks, and
networkx conversion.
