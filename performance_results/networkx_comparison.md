# ironweaver vs networkx

Generated 2026-09-28 19:15 by `benchmarks/compare_networkx.py`.

| | |
|---|---|
| Python | 3.11.15 (CPython) |
| networkx | 3.6.1 |
| Platform | Linux-6.18.44-fc-v37-x86_64-with-glibc2.39 |
| CPU cores | 4 |
| Repeats | best of 3 |
| Seed | 42 |

Both libraries get the same random directed multigraph (random edges plus a
`n0 → n1 → …` chain so everything is reachable from `n0`). Each cell is the
best wall-clock time over the repeats, and every row checks that both
libraries return the same result. "Speedup" is networkx time ÷ ironweaver time.

## 1,000 nodes, 5,999 edges

| Operation | Description | ironweaver | networkx | Speedup | Notes |
|---|---|---:|---:|---:|---|
| Build graph | add all nodes and edges one by one | 4.23 ms | 10.62 ms | **2.5× faster** |  |
| BFS (full) | all nodes reachable from `n0` | 1.10 ms | 1.01 ms | 1.1× slower | 1,000 nodes reached |
| BFS (depth 3) | depth-limited BFS | 200.9 µs | 144.8 µs | 1.4× slower | 194 nodes reached |
| BFS (edge filter) | only follow `type == "knows"` edges | 1.39 ms | 6.07 ms | **4.4× faster** | 683 nodes reached |
| DFS traversal | pre-order DFS from source | 1.24 ms | 1.82 ms | **1.5× faster** |  |
| BFS search | find `n999` from `n0` | 37.1 µs | 97.1 µs | **2.6× faster** | both use bidirectional BFS |
| Shortest path (unweighted) | BFS shortest path | 68.4 µs | 87.0 µs | **1.3× faster** | 5 hops; both use bidirectional BFS |
| Shortest path (Dijkstra) | weighted by `weight` attribute | 743.1 µs | 3.03 ms | **4.1× faster** | cost 14.95 |
| Grid: Dijkstra | 31×31 grid, left edge to right edge | 582.8 µs | 2.27 ms | **3.9× faster** | 721 nodes expanded |
| Grid: A* (coordinates) | Euclidean heuristic from `x`/`y` node attributes | 154.9 µs | 350.0 µs | **2.3× faster** | 51 nodes expanded; networkx calls a Python heuristic |
| Grid: A* (precomputed) | estimates from `vertex.meta` vs a dict lookup | 148.2 µs | 293.1 µs | **2.0× faster** | 51 nodes expanded |
| Subgraph by ids | 200 nodes + edges between them, as a new graph | 264.9 µs | 2.27 ms | **8.6× faster** |  |
| Subgraph by attribute | nodes with `group == 3` | 150.3 µs | 937.7 µs | **6.2× faster** |  |
| Expand (depth 2) | 100 seeds + neighbourhood, as a new graph | 719.3 µs | 35.01 ms | **48.7× faster** | 966 nodes |
| Remove nodes | remove 50 nodes and their edges | 273.7 µs | 425.1 µs | **1.6× faster** |  |
| Count edges | `get_metadata()` vs `number_of_edges()` | 82.0 µs | 1.11 ms | **13.5× faster** |  |
| Random walks | 5,000 walks of length 20, deduplicated | 8.14 ms | 33.76 ms | **4.1× faster** | networkx has no random walks; pure Python over its adjacency |
| Serialize to JSON string | `save_to_json()` vs `node_link_data` + `json.dumps` | 6.04 ms | 12.05 ms | **2.0× faster** |  |
| Load from JSON string | `load_from_json()` vs `json.loads` + `node_link_graph` | 7.49 ms | 17.63 ms | **2.4× faster** |  |

## 20,000 nodes, 119,999 edges

| Operation | Description | ironweaver | networkx | Speedup | Notes |
|---|---|---:|---:|---:|---|
| Build graph | add all nodes and edges one by one | 146.89 ms | 571.98 ms | **3.9× faster** |  |
| BFS (full) | all nodes reachable from `n0` | 68.24 ms | 49.76 ms | 1.4× slower | 20,000 nodes reached |
| BFS (depth 3) | depth-limited BFS | 570.0 µs | 422.5 µs | 1.3× slower | 329 nodes reached |
| BFS (edge filter) | only follow `type == "knows"` edges | 76.6 µs | 2.64 ms | **34.4× faster** | 3 nodes reached |
| DFS traversal | pre-order DFS from source | 55.62 ms | 73.69 ms | **1.3× faster** |  |
| BFS search | find `n19999` from `n0` | 184.3 µs | 369.2 µs | **2.0× faster** | both use bidirectional BFS |
| Shortest path (unweighted) | BFS shortest path | 245.6 µs | 358.6 µs | **1.5× faster** | 6 hops; both use bidirectional BFS |
| Shortest path (Dijkstra) | weighted by `weight` attribute | 18.06 ms | 61.95 ms | **3.4× faster** | cost 16.85 |
| Grid: Dijkstra | 141×141 grid, left edge to right edge | 21.61 ms | 80.96 ms | **3.7× faster** | 15,186 nodes expanded |
| Grid: A* (coordinates) | Euclidean heuristic from `x`/`y` node attributes | 2.39 ms | 6.38 ms | **2.7× faster** | 987 nodes expanded; networkx calls a Python heuristic |
| Grid: A* (precomputed) | estimates from `vertex.meta` vs a dict lookup | 2.42 ms | 5.87 ms | **2.4× faster** | 987 nodes expanded |
| Subgraph by ids | 4,000 nodes + edges between them, as a new graph | 7.00 ms | 70.93 ms | **10.1× faster** |  |
| Subgraph by attribute | nodes with `group == 3` | 2.99 ms | 22.70 ms | **7.6× faster** |  |
| Expand (depth 2) | 100 seeds + neighbourhood, as a new graph | 8.03 ms | 123.58 ms | **15.4× faster** | 3,950 nodes |
| Remove nodes | remove 1,000 nodes and their edges | 8.84 ms | 17.52 ms | **2.0× faster** |  |
| Count edges | `get_metadata()` vs `number_of_edges()` | 1.07 ms | 42.16 ms | **39.5× faster** |  |
| Random walks | 5,000 walks of length 20, deduplicated | 21.51 ms | 229.01 ms | **10.6× faster** | networkx has no random walks; pure Python over its adjacency |
| Serialize to JSON string | `save_to_json()` vs `node_link_data` + `json.dumps` | 275.39 ms | 405.54 ms | **1.5× faster** |  |
| Load from JSON string | `load_from_json()` vs `json.loads` + `node_link_graph` | 265.19 ms | 712.50 ms | **2.7× faster** |  |
