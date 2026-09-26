# AGENTS Instructions

## Codebase Structure
- The core Rust library lives in `src/` at the repository root.
  - `Cargo.toml` and `pyproject.toml` configure the PyO3 build.
  - Python sources are in `python/ironweaver/`.
  - Examples live in the `examples/` directory.
- Rust source files are in `src/`:
  - `lib.rs` exposes the Python module and re-exports structs.
  - `node.rs`, `edge.rs`, `path.rs` implement the main types.
  - `vertex/` contains logic for the `Vertex` class.
    - `core.rs` defines methods like `add_node`, `add_edge`, `expand`, etc.
    - `algorithms/` holds algorithm implementations such as BFS, random walks, expand and filter.
    - `analysis.rs`, `serialization.rs`, `manipulation.rs` provide auxiliary features.
- Python helper utilities live at repo root (e.g., `embedding_utils.py`).
- Tests are under `tests/` and rely on the compiled `ironweaver` module.

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
```

## Function Reference
Below is a quick guide to notable functions and where to find them.

- **embedding_utils.py**
  - `attach_embeddings_from_meta` – copy embeddings from `vertex.meta` to nodes.

- **src/node.rs**
  - `Node::new`, `__repr__`, `traverse` (exposed to Python as `_traverse`
    and wrapped in `__init__.py`), `bfs`, `bfs_search`,
    `attr_get`, `attr_set`, `attr_list_append`.

- **src/edge.rs**
  - `Edge::new`, `__repr__`, `toJSON`.

- **src/path.rs**
  - `Path::new`, `__repr__`, `toJSON`.

- **src/vertex/core.rs**
  - Constructors: `new`, `from_nodes`, `from_nodes_with_path`.
  - Graph methods: `add_node`, `add_edge`, `remove_node`, `remove_edge`,
    `get_node`, `has_node`, `node_count`, `prune`.
  - IO: `save_to_json`, `save_to_binary`, `save_to_binary_f16`, `load_from_json`, `load_from_binary`.
  - Analysis: `get_metadata`, `to_networkx`.
  - Algorithms: `shortest_path_bfs`, `shortest_path_dijkstra`, `expand`, `filter`, `random_walks`.
  - GC support: `__traverse__` / `__clear__` (also on `Node`, `Edge`, `Path`).

- **src/vertex/analysis.rs**
  - `get_metadata`, `to_networkx`.

- **src/vertex/manipulation.rs**
  - `add_node`, `add_edge`, `get_node`, `prune`, `remove_node`, `remove_edge`.

- **src/vertex/subgraph.rs**
  - `build_subgraph` (shared by filter/expand/shortest paths), `wire_vertex`,
    `neighbors`, `Direction`.

- **src/gc_pause.rs**
  - `GcPause` guard that pauses Python's cyclic GC during bulk object creation.

- **src/vertex/serialization.rs**
  - `save_to_json`, `save_to_binary`, `save_to_binary_f16`, `load_from_json`, `load_from_binary`.

- **src/vertex/algorithms/**
  - `expand.rs`: `expand`
  - `filter.rs`: `filter`
  - `random_walks.rs`: `random_walks`
  - `shortest_path_bfs.rs`: `shortest_path_bfs`
  - `shortest_path_dijkstra.rs`: `shortest_path_dijkstra`

- **src/serialization.rs**
  - On-disk format (JSON via sonic-rs, binary via bincode); legacy files in
    `tests/data/` must keep loading.
  - Saving: `GraphView` streams the live graph into the serializer
    (`to_json`, `write_binary`); `PyValue` encodes Python values.
  - Loading: `LoadGraph::from_json_slice` / `from_binary_slice` parse into
    borrowed structs, `LoadGraph::into_vertex` builds the Python objects;
    `dict_to_json` handles dict input.

- **src/observed_dictionary.rs**
  - `ObservedDictionary::new`, `__setitem__`, `__getitem__`.

- **python/ironweaver/lgf_parser.py**
  - `parse_lgf`, `parse_lgf_file`

