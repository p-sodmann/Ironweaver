# AGENTS Instructions

## Codebase Structure
- The repository is a Cargo workspace with two crates:
  - `crates/ironweaver-core/` — the pure-Rust core: graph storage and every
    algorithm. It must never depend on `pyo3` or use Python types
    (`cargo tree -p ironweaver-core | grep pyo3` must stay empty).
  - The root crate (`src/`) — the PyO3 bindings (`_ironweaver` extension):
    Python classes, conversion of Python values, callbacks and errors.
  - `Cargo.toml` and `pyproject.toml` at the root configure the PyO3 build.
  - Both crates use edition 2024; `rustfmt.toml` keeps `style_edition = "2021"`
    so the formatting (import order, line breaks) stays as it was.
- Python sources are in `python/ironweaver/`.
- Examples live in the `examples/` directory.
- Python helper utilities live at repo root (e.g., `embedding_utils.py`).
- Tests are under `tests/` (Python, need the compiled `ironweaver` module) and
  `crates/ironweaver-core/tests/` plus unit tests in the core (`cargo test`).

### Data model
- The core `Graph<N, E>` owns all nodes and edges (slot arenas addressed by
  `NodeIx` / `EdgeIx`; a handle to something removed never resolves again).
  Every edge is listed once in its source's out-list and once in its
  target's in-list. Handles are per process; what persists is node ids,
  `EdgeId`s (per-graph counter, never reused), node labels and edge types
  (interned `Symbol`s, with a label -> nodes index).
- In Python, `"labels"` (nodes) and `"type"` (edges) are reserved attribute
  names mapped onto those fields (`data::split_reserved`, `node_value` /
  `edge_value`): `add_node` / `add_edge`, `attr` getters and setters,
  `attr_get` / `attr_set`, dict filters and `remove_edge` all go through
  them. New code that reads or matches attributes by name must too.
- The bindings store `Graph<NodeData, EdgeData>` inside the Python `Vertex`;
  attribute values are Python objects. Python `Node` / `Edge` objects are
  frozen handles `(vertex, index)`: they compare with `==`, and every getter
  borrows the vertex. Never call back into Python while holding
  `borrow_mut()` of a Vertex.
- Core algorithms read payloads only through the `Attributes` trait and take
  callbacks returning `Result<_, X>`; the bindings use `errors::Error`
  (wraps `PyErr`, converts from `GraphError`) as `X`.

## Building and Installing
To build the Rust extension and install the package in editable mode:
```bash
pip install maturin
pip install -e .
```
`maturin` will compile the Rust code and create the Python module.

## Running Tests
After installing, run:
```bash
pytest
cargo test -p ironweaver-core
```
`tests/test_docs_examples.py` executes every ```python block in `README.md`,
`llms.txt` and `docs/*.md`, so documentation examples must run as written
(use a ```text fence for signatures or sketches). When you change the public
API, update `llms.txt`, the `.pyi` stubs and the docs together.

CI (`.github/workflows/ci.yml`) also runs `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo audit`
(ignored advisories, with reasons, are in `.cargo/audit.toml`), a check that
the core does not depend on pyo3, and builds with the latest stable Rust (the minimum
supported version is 1.99, `rust-version` in both Cargo.toml files; raise it
when a newer std API or language feature is worth using), rustdoc with
`-D warnings` plus `cargo package` for the core (its README is the crate
docs, so its example runs as a doctest), and pytest on Python 3.9-3.14
(Linux) plus macOS and Windows. Run fmt and clippy before pushing.

Docs site: `mkdocs.yml` (MkDocs Material; `pip install -r docs/requirements.txt`,
then `mkdocs serve`), published to GitHub Pages by `.github/workflows/docs.yml`,
which builds with `--strict` (broken links fail). A new page in `docs/` goes
into the `nav` of `mkdocs.yml`. `docs/api.md` is generated from the `.pyi`
stubs (mkdocstrings), so keep their docstrings current; `docs/benchmarks.md`
and `docs/changelog.md` include files from the repo.

Releases: see RELEASING.md (version in Cargo.toml, CHANGELOG.md, a `v*` tag
builds and publishes wheels and the core crate).

## Function Reference
Below is a quick guide to notable functions and where to find them.

- **embedding_utils.py**
  - `attach_embeddings_from_meta` – copy embeddings from `vertex.meta` to nodes.

### Core crate (`crates/ironweaver-core/src/`)

- **graph.rs** – `Graph<N, E>` (node ids in a `StrMap`: foldhash, seeded
  per process), `Node`, `Edge`, `NodeIx`, `EdgeIx`,
  `EdgeId`, `Symbol` / `Symbols`: `add_node`, `add_edge`, `insert_edge`
  (explicit id / type), `remove_node`, `remove_edge`, `rename_node`,
  `node_ix`, `edge_ix` (by `EdgeId`; `EdgeIndex`: dense table + sparse
  map), `next_edge_id` / `reserve_edge_ids`, `add_label` / `remove_label` /
  `nodes_with_label` / `labels`, `set_edge_type` / `edge_type_name`,
  `edge_types` / `edge_type_count` (a count per type, kept on every edge
  add / remove / retype), `edges_between`,
  `nodes`, `edges`, `neighbors`, `induced_subgraph` (shared by
  filter/expand/shortest paths/traversals; keeps ids, labels, types).
  `memory_usage` is O(1): the `heap` counter tracks each node's id, labels
  and adjacency lists, the id index keys and the label sets (and each
  property index its keys and postings). Code that changes those must
  update the counter (capacity before / after, also on removal from a hash
  set) or call `recount`; a randomized test checks it against a recount.
- **index.rs** – node property indexes owned by `Graph` (`BTreeMap<Key,
  Posting>` + each node's key): `create_index` / `create_index_with_keys`,
  `find_nodes`, `find_nodes_in_range`, `index_stats` (O(1) sizes),
  `index_candidates(Expr)` (used by the matcher and `filter`) =
  `index_plan` (`IndexPlan`, picked by `index_plan_estimate`, no postings
  read) + `execute_index_plan`, `set_index_keys` / `reindex_node` /
  `flush_indexes`. `add_node`, `node_mut`, `nodes_mut` mark nodes dirty;
  lookups re-read dirty nodes, so they are exact before a flush.
  `begin_index_build` -> `IndexBuild` (filled through `&Graph`) ->
  `install_index` (O(changed): while a build is open the graph records
  node changes in `Indexes::touch` / `touch_all` / `remove`).
- **temporal.rs** – `Date`, `DateTime` (offset or wall-clock), `Parts`, and
  the `bytes` serde helper (base64 in JSON); `value.rs` has `Key` (hashable,
  totally ordered scalar keys agreeing with `loose_eq` / `loose_cmp`).
- **ops.rs** – `Op<N, E>` (changes as data, nodes by id, edges by
  `EdgeId`; serde), `Graph::apply` (checks first, returns the undo ops),
  `apply_all` (atomic; a failing undo is `GraphError::Internal`, never a
  panic), `AttrPatch` (per-attribute ops; `Record` has it).
- **expr.rs** – `Expr` / `CmpOp`: filter expressions (compare / in / exists
  on attribute paths, `Label`, `Type`, and / or / not), `matches_node` /
  `matches_edge`, read through `Attributes::with_value`. Missing values make
  comparisons false. Serde (externally tagged), with `And` / `Or` / `Not`
  nesting capped at `MAX_EXPR_DEPTH` both ways (the bindings use it too)
  and unknown fields refused (skipping one recurses past the depth
  counters), like the pattern structs; `Expr` / `Value` / `Pattern`
  `from_json_str` read JSON with no parser recursion limit, relying on that.
- **error.rs** – `GraphError` (`#[non_exhaustive]`; its `Display` text is
  the user-facing message; `BudgetExceeded`, `Internal` for broken
  invariants).
- **budget.rs** – `Budget` (`max_visited`, `max_edges`, `max_results`,
  `OnLimit`), `Limited<T>` (`value`, `truncated`, `visited`, `edges`) and
  the crate-internal `Meter` that searches count with (`enter` / `examine`
  per edge / `produce`, `finish`). Searches poll cancellation per edge.
- **cancel.rs** – cancellation: `Token`, `run` (runs a closure under a
  token; `Err(Interrupted)` if cancelled), `run_polling` (with a hook the
  sequential loops call now and then), `stop()` -> `Stop`: parallel loops
  check `requested()`, sequential searches `poll()`. A new long-running
  loop must check one and bail out early (the partial result is dropped).
  Progress the same way: `run_with_progress` installs a `Progress`
  (phase, done, total; read with `snapshot` from any thread), algorithms
  fetch `progress()` -> `Report` and call `start` / `add` / `tick` where
  they check the stop flag.
- **direction.rs** – `Direction` (`"out"`, `"in"`, `"both"`).
- **value.rs**, **record.rs** – `Value`, `Record` (payload for pure-Rust
  graphs), the `Attributes` trait, `Lookup`, and `lookup` (the attribute
  path rules on an `Attrs` map; `Record` uses it).
- **traversal.rs** – `dfs`, `bfs`, `expand` (multi-source BFS), their
  `*_limited` variants under a `Budget`, `bidirectional_bfs` (used by the
  `bfs` path method and `Node.bfs_search`).
- **pathfinding/** (everything behind `Vertex.shortest_path`)
  - `mod.rs`: `find_path` entry point, `PathMethod` + the `METHODS`
    registry, `resolve` / `check_options` / `edge_cost` / `check_max_cost`,
    `PathQuery`, `PathResult`, `not_reachable`, `find_path_limited` (a
  `Meter` is passed down to each method). The comment at its top is the
    recipe for adding an algorithm.
  - `bfs.rs`, `dijkstra.rs`, `astar.rs`: one `METHOD` each.
  - `best_first.rs`: best-first search shared by Dijkstra and A* (reopening
    for admissible heuristics, `expanded` count).
  - `cost.rs`: `EdgeCost` (weight attribute, default, validation).
  - `heuristic.rs`: `Heuristic` (zero, node coordinates, custom closure),
    `Metric`, `Coords`.
- **projection.rs** – `Projection`: compact read-only copy of (part of) a
  graph for analytics: CSR adjacency with sorted neighbour lists, optional
  weights, owned node ids, no payloads (`Send + Sync`, a snapshot). Built in
  two steps: `Projection::collect` (reads the graph: node / edge filters,
  weights; one pass over the edge arena, rows by counting sort) then
  `RawProjection::finish` (sorts rows; no graph access, so the
  bindings release the GIL); `Projection::build` does both. The transposed
  adjacency (`in_neighbors`) and the id index (`index_of_id`) are built
  lazily. New analytics algorithms take `&Projection` and dense `u32` node
  indices.
- **batch.rs** – `shortest_paths` / `distances` on a `Projection`: many
  queries in parallel (rayon, per-thread workspaces), Dijkstra or BFS.
- **algo/** – analytics on a `Projection` (dense indices in, `Vec`s / groups
  out; the comment in `mod.rs` is the recipe for adding one). Tests compare
  against brute-force references (`algo::testing` graphs); Python tests in
  `tests/test_algorithms.py` compare against networkx.
  - `mod.rs`: `Undirected` (simple undirected view: sorted, deduplicated,
    no self-loops), `groups` (largest first), `NONE`.
  - `components.rs`: `weakly_connected_components` (union-find),
    `strongly_connected_components` (iterative Tarjan).
  - `dag.rs`: `topological_sort` (Kahn, min-heap), `find_cycle`.
  - `centrality.rs`: `degree_centrality`, `pagerank` + `PageRank` options
    (parallel pull; networkx semantics).
  - `structure.rs`: `triangles` / `clustering` (degree-ordered: each
    triangle once, from its lowest-degree node), `clustering_directed`
    (LDBC LCC), `core_number`.
  - `community.rs`: `label_propagation` (synchronous LDBC CDLP: on
    directed projections in- and out-neighbours count separately).
  - `bfs.rs`: `bfs_levels` (parallel, direction-optimizing).
  - `sssp.rs`: `Search` (reusable single-source BFS / Dijkstra, forwards or
    backwards) and `SimpleRow` (lightest of parallel edges, no self-loops),
    shared by the shortest-path based algorithms.
  - `betweenness.rs`: `betweenness_centrality` + `Betweenness` options
    (Brandes, parallel over sources; `sample_sources` for estimates;
    networkx scaling).
  - `closeness.rs`: `closeness_centrality`, `harmonic_centrality`.
  - `similarity.rs`: `Similarity` metrics, `similarity` (pairs),
    `most_similar` (top k per node through common neighbours).
  - `leiden.rs`: `leiden` + `Leiden` options (local moving, refinement,
    aggregation in linear time and in parallel; repeated from its own
    result, `max_iter` 3 by default), `modularity`.
  - `spanning.rs`: `spanning_forest` (Kruskal).
  - `ksp.rs`: `k_shortest_paths` (Yen).
  - `embedding.rs`: `fastrp` + `FastRP` options.
  - `node2vec.rs`: `node2vec_walks` + `Node2Vec` options (rejection
    sampling).
  - `mod.rs` helpers: `ordered_sum` (parallel sums independent of the
    thread count), `mix` (per-item seeds), `random_seed`.
- **query/** – query primitives for a query engine.
  - `paths.rs`: `expand_paths` (variable-length paths, `Hops` min / max,
    `Uniqueness` walk / trail / path, streamed to a visitor),
    `expand_paths_limited` (under a `Budget`), `steps` (one step in a
    direction; `Both` lists self-loops once).
  - `pattern.rs`: `Pattern` / `NodePattern` / `EdgePattern` (serde),
    `Pattern::parse` (Cypher-like text: labels, types, `*min..max`,
    `{key: value}`), `add_filter`, `bind_ids`; `Display` / `to_text`
    write text that parses back to an equal pattern (`TextWriter` picks an
    order that reproduces the node numbering; a random test checks the
    round trip) and mark what text can't express in `<...>`.
  - `matcher.rs`: `for_each_match` / `find_matches` (backtracking; plan
    from the most selective node; edges distinct per match, nodes may
    repeat; variable-length edges streamed), their `*_limited` variants,
    `Match`, `Bound`. Tests compare with brute force.
- **random_walks.rs** – `WalkOptions`, `plan` / `plan_limited` (from a
  start node, indexes only what walks reach) → `WalkPlan::run` (no graph
  access, so the bindings release the GIL) / `run_limited` (a `Budget`
  caps the attempts in advance) / `WalkPlan::items`, `random_walks`
  convenience.
- **format/** – on-disk format version 2 (JSON via sonic-rs; binary:
  header + postcard payload + trailer with length and CRC32; layout in the
  comment at the top of `mod.rs`). Version 1 JSON files ("1.x") keep
  loading: `migrate_v1` in `load.rs` (attr "labels" / "type" -> fields),
  new edge ids (Python keeps the old one in `meta["legacy_id"]`);
  `tests/data/legacy_graph.json` (v1) and `tests/data/v2_*` (v2 golden
  files) must keep loading. Version 1 binary files (headerless bincode,
  `tests/data/legacy_*.bin`) are refused with a clear error (`unframed`). The
  binary field order is positional: save.rs and load.rs structs must match.
  postcard drops custom error messages, so raise them with `ser_error` /
  `de_error`.
  - `save.rs`: `GraphWriter` streams a graph into the serializer, payloads
    encoded by a `Codec` (`RecordCodec` for `Record`, keys sorted so
    saves are deterministic); `with_timestamp`; `tagged` encoders.
  - `load.rs`: `LoadGraph::from_json_slice` / `from_binary_slice` parse into
    borrowed structs, `LoadGraph::build` makes the `Graph`.
    `metadata.indexes` (written by `save.rs` only if there are indexes)
    holds the property index paths; `restore_indexes` recreates them empty
    and dirty, the `Record` loaders flush them and the bindings `reindex`.
  - `stream.rs`: `LoadGraph::build_from_reader` decodes a binary file from
    a `Read` (postcard flavor `Framed`: CRC32 of all but the last 16 bytes,
    trailer checked at the end) and builds the graph entry by entry;
    strings are read owned there (`owned_strings`), so loader types must
    not borrow from the input in any other way.
  - `mod.rs`: `to_json` / `to_binary` / `from_json` / `from_binary` /
    `from_binary_reader` for `Graph<Record, Record>`; `write_atomic` (temp
    file + fsync + rename, used by every file save).
  - Values nest at most `MAX_DEPTH` (100) levels: `LoadValue` rejects deeper
    input (so crafted files cannot overflow the stack) and savers check with
    `tagged::check_depth`. JSON graph documents nested more than 255 levels
    (sonic-rs skips unknown fields recursively) are refused up front by
    `check_json_nesting`. Any new recursive (de)serializer must do the same.

### Bindings (`src/`)

- **lib.rs** – exposes the Python module and re-exports the classes.
- **data.rs** – `NodeData` / `EdgeData` payloads (Python attribute values),
  `PyGraph`, their `Attributes` impls; reserved-name helpers
  `split_reserved`, `node_value`, `edge_value`.
- **errors.rs** – `graph_error` (GraphError → Python exception), `Error`.
- **convert.rs** – `PyCodec` (Python values → tagged values while saving),
  `to_python` / `to_attr_map` (loaded values → Python, string sharing),
  `dict_to_json`, `to_value` (Python → core `Value`, for expressions).
- **expr.rs** – `Expr` / `Attr` classes and `attr` / `label` / `edge_type`
  (build core `Expr`s; guards against `and` / `or` / chained comparisons and
  missing parentheses; nesting capped, `&` / `|` chains flattened).
- **node.rs** – `Node` handle: getters/setters, `labels` / `add_label` /
  `remove_label` / `has_label`, `_traverse` (wrapped as `traverse` in
  `__init__.py`), `bfs`, `bfs_search`, `paths`, `attr_get`, `attr_set`,
  `attr_list_append`; `edge_predicate` (dict / callable edge filters).
- **edge.rs** – `Edge` handle: `id` (the `EdgeId`), `type`, getters/setters,
  `attr_get`, `attr_set`, `toJSON`.
- **path.rs** – `Path` (nodes and edges of a path; `Node.paths`). **observed_dictionary.rs** – `ObservedDictionary`.
- **projection.rs** – the `Projection` class (queries and analytics
  methods release the GIL),
  `Filter` (dict / callable node and edge filters), `collect` (Vertex →
  core `RawProjection`), result conversion shared with `vertex/batch.rs`.
  `Vertex.project` is wrapped in `__init__.py` so callable filters receive
  `NodeView` / `EdgeView`.
- **interrupt.rs** – Ctrl+C: `released(py, size, f)` runs GIL-free work on
  rayon's pool under a token while the caller checks signals every 20 ms
  (inline below 20,000 nodes + edges); `polling(py, f)` runs GIL-held
  searches with a hook that checks signals. Every long computation goes
  through one of them.
- **gc_pause.rs** – `GcPause` guard that pauses Python's cyclic GC during bulk
  object creation.
- **vertex/core.rs** – the `Vertex` class: constructors (`new`, `from_nodes`,
  `from_nodes_with_path`), `add_node` (`labels=`), `add_edge` (`type=`),
  `add_nodes` / `add_edges` (bulk, in `bulk.rs`), `get_node`, `get_edge`,
  `nodes_with_label`, `match`, `has_node`,
  `node_count`, `nodes`, GC support (`__traverse__` / `__clear__`), and thin
  wrappers around the modules below.
- **vertex/manipulation.rs** – `remove_node`, `remove_edge`.
- **vertex/bulk.rs** – `add_nodes` / `add_edges` (read and check every item
  first, then insert all; attribute columns; GC paused).
- **vertex/algorithms.rs** – `expand`, `filter`, `random_walks`.
- **vertex/pathfinding.rs** – `shortest_path` (Python options → core
  `PathQuery`, `distances=` table heuristic), `path_methods`.
- **vertex/batch.rs** – `project`, and `shortest_paths` / `distances` (a
  one-off projection of the whole graph, queries with the GIL released).
- **vertex/index.rs** – `create_index`, `find`, `find_range`, and
  `reindex(py, vertex, nodes)`: after changing node attributes, call it
  (keys are computed with no mutable borrow held, since reading a value can
  run Python code).
- **vertex/subgraph.rs** – `build_subgraph` (new Vertex from part of another).
- **vertex/serialization.rs** – `save_to_json`, `save_to_binary`,
  `save_to_binary_f16`, `load_from_json`, `load_from_binary`.
- **vertex/analysis.rs** – `get_metadata`, `memory_usage` (`deep`: getsizeof
  of attribute dicts and values, outside the borrow), `to_networkx`.
- **vertex/callbacks.rs** – `fire` (runs a callback list).
- **vertex/query.rs** – `match_pattern` (`Vertex.match`: `where` Exprs,
  `ids`, results as Node / Edge / list[Edge]) and `node_paths`
  (`Node.paths`, returns `Path`s).

- **python/ironweaver/lgf_parser.py**
  - `parse_lgf`, `parse_lgf_file`
