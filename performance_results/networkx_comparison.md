# ironweaver vs networkx

Generated 2026-09-26 10:45 by `benchmarks/compare_networkx.py`.

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
| Build graph | add all nodes and edges one by one | 4.43 ms | 10.34 ms | **2.3× faster** |  |
| BFS (full) | all nodes reachable from `n0` | 1.02 ms | 845.0 µs | 1.2× slower | 1,000 nodes reached |
| BFS (depth 3) | depth-limited BFS | 181.0 µs | 139.9 µs | 1.3× slower | 194 nodes reached |
| BFS (edge filter) | only follow `type == "knows"` edges | 1.05 ms | 6.51 ms | **6.2× faster** | 683 nodes reached |
| DFS traversal | pre-order DFS from source | 1.63 ms | 1.50 ms | 1.1× slower |  |
| BFS search | find `n999` from `n0` | 52.9 µs | 92.3 µs | **1.7× faster** | both use bidirectional BFS |
| Shortest path (unweighted) | BFS shortest path | 76.4 µs | 73.5 µs | 1.0× slower | 5 hops; both use bidirectional BFS |
| Shortest path (Dijkstra) | weighted by `weight` attribute | 919.1 µs | 3.39 ms | **3.7× faster** | cost 14.95 |
| Grid: Dijkstra | 31×31 grid, left edge to right edge | 690.7 µs | 2.40 ms | **3.5× faster** | 721 nodes expanded |
| Grid: A* (coordinates) | Euclidean heuristic from `x`/`y` node attributes | 204.3 µs | 370.6 µs | **1.8× faster** | 51 nodes expanded; networkx calls a Python heuristic |
| Grid: A* (precomputed) | estimates from `vertex.meta` vs a dict lookup | 186.2 µs | 299.2 µs | **1.6× faster** | 51 nodes expanded |
| Subgraph by ids | 200 nodes + edges between them, as a new graph | 613.3 µs | 2.65 ms | **4.3× faster** |  |
| Subgraph by attribute | nodes with `group == 3` | 336.1 µs | 1.03 ms | **3.1× faster** |  |
| Expand (depth 2) | 100 seeds + neighbourhood, as a new graph | 6.21 ms | 42.07 ms | **6.8× faster** | 966 nodes |
| Remove nodes | remove 50 nodes and their edges | 248.5 µs | 393.3 µs | **1.6× faster** |  |
| Count edges | `get_metadata()` vs `number_of_edges()` | 233.5 µs | 1.36 ms | **5.8× faster** |  |
| Random walks | 5,000 walks of length 20, deduplicated | 6.62 ms | 32.27 ms | **4.9× faster** | networkx has no random walks; pure Python over its adjacency |
| Serialize to JSON string | `save_to_json()` vs `node_link_data` + `json.dumps` | 7.83 ms | 13.12 ms | **1.7× faster** |  |
| Load from JSON string | `load_from_json()` vs `json.loads` + `node_link_graph` | 7.55 ms | 16.97 ms | **2.2× faster** |  |

## 20,000 nodes, 119,999 edges

| Operation | Description | ironweaver | networkx | Speedup | Notes |
|---|---|---:|---:|---:|---|
| Build graph | add all nodes and edges one by one | 339.89 ms | 641.01 ms | **1.9× faster** |  |
| BFS (full) | all nodes reachable from `n0` | 60.25 ms | 46.16 ms | 1.3× slower | 20,000 nodes reached |
| BFS (depth 3) | depth-limited BFS | 682.5 µs | 458.0 µs | 1.5× slower | 329 nodes reached |
| BFS (edge filter) | only follow `type == "knows"` edges | 67.1 µs | 2.94 ms | **43.8× faster** | 3 nodes reached |
| DFS traversal | pre-order DFS from source | 82.66 ms | 74.52 ms | 1.1× slower |  |
| BFS search | find `n19999` from `n0` | 232.1 µs | 417.2 µs | **1.8× faster** | both use bidirectional BFS |
| Shortest path (unweighted) | BFS shortest path | 314.1 µs | 373.5 µs | **1.2× faster** | 6 hops; both use bidirectional BFS |
| Shortest path (Dijkstra) | weighted by `weight` attribute | 20.27 ms | 65.14 ms | **3.2× faster** | cost 16.85 |
| Grid: Dijkstra | 141×141 grid, left edge to right edge | 31.52 ms | 84.65 ms | **2.7× faster** | 15,186 nodes expanded |
| Grid: A* (coordinates) | Euclidean heuristic from `x`/`y` node attributes | 2.77 ms | 6.70 ms | **2.4× faster** | 987 nodes expanded; networkx calls a Python heuristic |
| Grid: A* (precomputed) | estimates from `vertex.meta` vs a dict lookup | 3.15 ms | 5.89 ms | **1.9× faster** | 987 nodes expanded |
| Subgraph by ids | 4,000 nodes + edges between them, as a new graph | 20.27 ms | 67.39 ms | **3.3× faster** |  |
| Subgraph by attribute | nodes with `group == 3` | 13.22 ms | 22.01 ms | **1.7× faster** |  |
| Expand (depth 2) | 100 seeds + neighbourhood, as a new graph | 23.14 ms | 92.85 ms | **4.0× faster** | 3,950 nodes |
| Remove nodes | remove 1,000 nodes and their edges | 10.37 ms | 17.50 ms | **1.7× faster** |  |
| Count edges | `get_metadata()` vs `number_of_edges()` | 7.17 ms | 54.53 ms | **7.6× faster** |  |
| Random walks | 5,000 walks of length 20, deduplicated | 70.19 ms | 227.18 ms | **3.2× faster** | networkx has no random walks; pure Python over its adjacency |
| Serialize to JSON string | `save_to_json()` vs `node_link_data` + `json.dumps` | 275.83 ms | 370.16 ms | **1.3× faster** |  |
| Load from JSON string | `load_from_json()` vs `json.loads` + `node_link_graph` | 274.82 ms | 681.71 ms | **2.5× faster** |  |
