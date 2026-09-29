# AGENTS Instructions

## Codebase Structure
- The repository is a Cargo workspace with two crates:
  - `crates/ironweaver-core/` — the pure-Rust core: graph storage and every
    algorithm. It must never depend on `pyo3` or use Python types
    (`cargo tree -p ironweaver-core | grep pyo3` must stay empty).
  - The root crate (`src/`) — the PyO3 bindings (`_ironweaver` extension):
    Python classes, conversion of Python values, callbacks and errors.
  - `Cargo.toml` and `pyproject.toml` at the root configure the PyO3 build.
- Python sources are in `python/ironweaver/`.
- Examples live in the `examples/` directory.
- Python helper utilities live at repo root (e.g., `embedding_utils.py`).
- Tests are under `tests/` (Python, need the compiled `ironweaver` module) and
  `crates/ironweaver-core/tests/` plus unit tests in the core (`cargo test`).

### Data model
- The core `Graph<N, E>` owns all nodes and edges (slot arenas addressed by
  `NodeIx` / `EdgeIx`; a handle to something removed never resolves again).
  Every edge is listed once in its source's out-list and once in its
  target's in-list.
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
the core does not depend on pyo3, and pytest on Python 3.9-3.13 (Linux) plus
macOS and Windows. Run fmt and clippy before pushing.

## Function Reference
Below is a quick guide to notable functions and where to find them.

- **embedding_utils.py**
  - `attach_embeddings_from_meta` – copy embeddings from `vertex.meta` to nodes.

### Core crate (`crates/ironweaver-core/src/`)

- **graph.rs** – `Graph<N, E>`, `Node`, `Edge`, `NodeIx`, `EdgeIx`:
  `add_node`, `add_edge`, `remove_node`, `remove_edge`, `rename_node`,
  `node_ix`, `node`, `edge`, `nodes`, `edges`, `neighbors`,
  `induced_subgraph` (shared by filter/expand/shortest paths/traversals).
- **error.rs** – `GraphError` (its `Display` text is the user-facing message).
- **direction.rs** – `Direction` (`"out"`, `"in"`, `"both"`).
- **value.rs**, **record.rs** – `Value`, `Record` (payload for pure-Rust
  graphs), the `Attributes` trait and `Lookup`.
- **traversal.rs** – `dfs`, `bfs`, `expand` (multi-source BFS),
  `bidirectional_bfs` (used by the `bfs` path method and `Node.bfs_search`).
- **pathfinding/** (everything behind `Vertex.shortest_path`)
  - `mod.rs`: `find_path` entry point, `PathMethod` + the `METHODS`
    registry, `resolve` / `check_options` / `edge_cost` / `check_max_cost`,
    `PathQuery`, `PathResult`, `not_reachable`. The comment at its top is the
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
  weights) then `RawProjection::finish` (sorts; no graph access, so the
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
  - `structure.rs`: `triangles`, `clustering`, `core_number`.
  - `community.rs`: `label_propagation` (synchronous CDLP).
  - `bfs.rs`: `bfs_levels` (parallel, direction-optimizing).
- **random_walks.rs** – `WalkOptions`, `plan` → `WalkPlan::run` (no graph
  access, so the bindings release the GIL) / `WalkPlan::items`,
  `random_walks` convenience.
- **format/** – on-disk format (JSON via sonic-rs, binary via bincode);
  legacy files in `tests/data/` must keep loading.
  - `save.rs`: `GraphWriter` streams a graph into the serializer, payloads
    encoded by a `Codec` (`RecordCodec` for `Record`); `tagged` encoders.
  - `load.rs`: `LoadGraph::from_json_slice` / `from_binary_slice` parse into
    borrowed structs, `LoadGraph::build` makes the `Graph`.
  - `mod.rs`: `to_json` / `to_binary` / `from_json` / `from_binary` for
    `Graph<Record, Record>`; `write_atomic` (temp file + fsync + rename,
    used by every file save).
  - Values nest at most `MAX_DEPTH` (100) levels: `LoadValue` rejects deeper
    input (so crafted files cannot overflow the stack) and savers check with
    `tagged::check_depth`. Any new recursive (de)serializer must do the same.

### Bindings (`src/`)

- **lib.rs** – exposes the Python module and re-exports the classes.
- **data.rs** – `NodeData` / `EdgeData` payloads (Python attribute values),
  `PyGraph`, their `Attributes` impls.
- **errors.rs** – `graph_error` (GraphError → Python exception), `Error`.
- **convert.rs** – `PyCodec` (Python values → tagged values while saving),
  `to_python` / `to_attr_map` (loaded values → Python, string sharing),
  `dict_to_json`.
- **node.rs** – `Node` handle: getters/setters, `_traverse` (wrapped as
  `traverse` in `__init__.py`), `bfs`, `bfs_search`, `attr_get`, `attr_set`,
  `attr_list_append`; `edge_predicate` (dict / callable edge filters).
- **edge.rs** – `Edge` handle: getters/setters, `attr_get`, `attr_set`, `toJSON`.
- **path.rs** – `Path`. **observed_dictionary.rs** – `ObservedDictionary`.
- **projection.rs** – the `Projection` class (queries and analytics
  methods release the GIL),
  `Filter` (dict / callable node and edge filters), `collect` (Vertex →
  core `RawProjection`), result conversion shared with `vertex/batch.rs`.
  `Vertex.project` is wrapped in `__init__.py` so callable filters receive
  `NodeView` / `EdgeView`.
- **gc_pause.rs** – `GcPause` guard that pauses Python's cyclic GC during bulk
  object creation.
- **vertex/core.rs** – the `Vertex` class: constructors (`new`, `from_nodes`,
  `from_nodes_with_path`), `add_node`, `add_edge`, `get_node`, `has_node`,
  `node_count`, `nodes`, GC support (`__traverse__` / `__clear__`), and thin
  wrappers around the modules below.
- **vertex/manipulation.rs** – `remove_node`, `remove_edge`.
- **vertex/algorithms.rs** – `expand`, `filter`, `random_walks`.
- **vertex/pathfinding.rs** – `shortest_path` (Python options → core
  `PathQuery`, `distances=` table heuristic), `path_methods`.
- **vertex/batch.rs** – `project`, and `shortest_paths` / `distances` (a
  one-off projection of the whole graph, queries with the GIL released).
- **vertex/subgraph.rs** – `build_subgraph` (new Vertex from part of another).
- **vertex/serialization.rs** – `save_to_json`, `save_to_binary`,
  `save_to_binary_f16`, `load_from_json`, `load_from_binary`.
- **vertex/analysis.rs** – `get_metadata`, `to_networkx`.
- **vertex/callbacks.rs** – `fire` (runs a callback list).

- **python/ironweaver/lgf_parser.py**
  - `parse_lgf`, `parse_lgf_file`
