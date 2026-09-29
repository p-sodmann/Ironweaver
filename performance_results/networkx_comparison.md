# ironweaver vs networkx

Generated 2026-09-29 07:55 by `benchmarks/compare_networkx.py`.

| | |
|---|---|
| Python | 3.13.12 (CPython) |
| networkx | 3.7 |
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
| Build graph | add all nodes and edges one by one | 4.13 ms | 11.42 ms | **2.8× faster** |  |
| BFS (full) | all nodes reachable from `n0` | 993.0 µs | 767.7 µs | 1.3× slower | 1,000 nodes reached |
| BFS (depth 3) | depth-limited BFS | 191.3 µs | 140.3 µs | 1.4× slower | 194 nodes reached |
| BFS (edge filter) | only follow `type == "knows"` edges | 1.55 ms | 4.48 ms | **2.9× faster** | 683 nodes reached |
| DFS traversal | pre-order DFS from source | 936.9 µs | 1.22 ms | **1.3× faster** |  |
| BFS search | find `n999` from `n0` | 26.3 µs | 69.1 µs | **2.6× faster** | both use bidirectional BFS |
| Shortest path (unweighted) | BFS shortest path | 49.9 µs | 65.6 µs | **1.3× faster** | 5 hops; both use bidirectional BFS |
| Shortest path (Dijkstra) | weighted by `weight` attribute | 880.6 µs | 3.11 ms | **3.5× faster** | cost 14.95 |
| Batch shortest paths | 64 Dijkstra pairs in one `shortest_paths` call | 2.48 ms | 211.87 ms | **85.4× faster** | one `shortest_path` call per pair: 20.31 ms |
| Distances from sources | Dijkstra from 16 sources to every reachable node | 2.60 ms | 86.33 ms | **33.3× faster** | 16,000 distances |
| Project graph | `project(weight=...)`: compact read-only copy for analytics | 724.5 µs | – | – | 0.1 MB |
| Weakly connected components | `project().weakly_connected_components()` | 438.2 µs | 909.0 µs | **2.1× faster** | 1 components |
| Strongly connected components | `project().strongly_connected_components()` | 348.1 µs | 1.40 ms | **4.0× faster** | 1 components |
| PageRank | weighted, `project(weight=...).pagerank()` | 1.53 ms | 6.29 ms | **4.1× faster** |  |
| Triangles | per node, edges as undirected (networkx: incl. `nx.Graph` conversion) | 1.42 ms | 37.92 ms | **26.7× faster** | 279 triangles |
| Core number | k-core decomposition (networkx: incl. `nx.Graph` conversion) | 917.6 µs | 33.83 ms | **36.9× faster** | max core 8 |
| Label propagation | communities; networkx uses its semi-synchronous variant | 1.98 ms | 48.73 ms | **24.6× faster** | 1 vs 1 communities |
| BFS levels | hop distance from one node to all (parallel, direction-optimizing) | 1.17 ms | 708.6 µs | 1.7× slower | 1,000 reached |
| Grid: Dijkstra | 31×31 grid, left edge to right edge | 586.3 µs | 2.68 ms | **4.6× faster** | 721 nodes expanded |
| Grid: A* (coordinates) | Euclidean heuristic from `x`/`y` node attributes | 132.3 µs | 341.6 µs | **2.6× faster** | 51 nodes expanded; networkx calls a Python heuristic |
| Grid: A* (precomputed) | estimates from `vertex.meta` vs a dict lookup | 146.1 µs | 263.5 µs | **1.8× faster** | 51 nodes expanded |
| Subgraph by ids | 200 nodes + edges between them, as a new graph | 264.9 µs | 2.59 ms | **9.8× faster** |  |
| Subgraph by attribute | nodes with `group == 3` | 199.0 µs | 1.36 ms | **6.8× faster** |  |
| Expand (depth 2) | 100 seeds + neighbourhood, as a new graph | 1.33 ms | 48.63 ms | **36.4× faster** | 966 nodes |
| Remove nodes | remove 50 nodes and their edges | 212.2 µs | 401.6 µs | **1.9× faster** |  |
| Count edges | `get_metadata()` vs `number_of_edges()` | 73.8 µs | 1.15 ms | **15.5× faster** |  |
| Random walks | 5,000 walks of length 20, deduplicated | 6.47 ms | 34.78 ms | **5.4× faster** | networkx has no random walks; pure Python over its adjacency |
| Serialize to JSON string | `save_to_json()` vs `node_link_data` + `json.dumps` | 5.05 ms | 8.53 ms | **1.7× faster** |  |
| Load from JSON string | `load_from_json()` vs `json.loads` + `node_link_graph` | 7.01 ms | 16.13 ms | **2.3× faster** |  |

## 20,000 nodes, 119,999 edges

| Operation | Description | ironweaver | networkx | Speedup | Notes |
|---|---|---:|---:|---:|---|
| Build graph | add all nodes and edges one by one | 122.18 ms | 655.08 ms | **5.4× faster** |  |
| BFS (full) | all nodes reachable from `n0` | 46.09 ms | 38.21 ms | 1.2× slower | 20,000 nodes reached |
| BFS (depth 3) | depth-limited BFS | 558.6 µs | 376.9 µs | 1.5× slower | 329 nodes reached |
| BFS (edge filter) | only follow `type == "knows"` edges | 56.4 µs | 2.52 ms | **44.6× faster** | 3 nodes reached |
| DFS traversal | pre-order DFS from source | 41.53 ms | 53.22 ms | **1.3× faster** |  |
| BFS search | find `n19999` from `n0` | 162.3 µs | 271.9 µs | **1.7× faster** | both use bidirectional BFS |
| Shortest path (unweighted) | BFS shortest path | 214.4 µs | 277.8 µs | **1.3× faster** | 6 hops; both use bidirectional BFS |
| Shortest path (Dijkstra) | weighted by `weight` attribute | 12.00 ms | 49.05 ms | **4.1× faster** | cost 16.85 |
| Batch shortest paths | 64 Dijkstra pairs in one `shortest_paths` call | 42.59 ms | 7.267 s | **170.6× faster** | one `shortest_path` call per pair: 1.027 s |
| Distances from sources | Dijkstra from 16 sources to every reachable node | 69.25 ms | 3.747 s | **54.1× faster** | 320,000 distances |
| Project graph | `project(weight=...)`: compact read-only copy for analytics | 10.94 ms | – | – | 2.3 MB |
| Weakly connected components | `project().weakly_connected_components()` | 9.05 ms | 37.57 ms | **4.1× faster** | 1 components |
| Strongly connected components | `project().strongly_connected_components()` | 9.79 ms | 56.24 ms | **5.7× faster** | 1 components |
| PageRank | weighted, `project(weight=...).pagerank()` | 17.18 ms | 201.32 ms | **11.7× faster** |  |
| Triangles | per node, edges as undirected (networkx: incl. `nx.Graph` conversion) | 21.51 ms | 1.068 s | **49.7× faster** | 247 triangles |
| Core number | k-core decomposition (networkx: incl. `nx.Graph` conversion) | 13.90 ms | 1.063 s | **76.5× faster** | max core 8 |
| Label propagation | communities; networkx uses its semi-synchronous variant | 17.20 ms | 2.126 s | **123.6× faster** | 1 vs 1 communities |
| BFS levels | hop distance from one node to all (parallel, direction-optimizing) | 13.56 ms | 32.26 ms | **2.4× faster** | 20,000 reached |
| Grid: Dijkstra | 141×141 grid, left edge to right edge | 17.60 ms | 80.43 ms | **4.6× faster** | 15,186 nodes expanded |
| Grid: A* (coordinates) | Euclidean heuristic from `x`/`y` node attributes | 1.87 ms | 6.29 ms | **3.4× faster** | 987 nodes expanded; networkx calls a Python heuristic |
| Grid: A* (precomputed) | estimates from `vertex.meta` vs a dict lookup | 1.97 ms | 5.71 ms | **2.9× faster** | 987 nodes expanded |
| Subgraph by ids | 4,000 nodes + edges between them, as a new graph | 5.92 ms | 67.39 ms | **11.4× faster** |  |
| Subgraph by attribute | nodes with `group == 3` | 2.70 ms | 18.95 ms | **7.0× faster** |  |
| Expand (depth 2) | 100 seeds + neighbourhood, as a new graph | 4.96 ms | 85.90 ms | **17.3× faster** | 3,950 nodes |
| Remove nodes | remove 1,000 nodes and their edges | 9.72 ms | 14.69 ms | **1.5× faster** |  |
| Count edges | `get_metadata()` vs `number_of_edges()` | 1.50 ms | 44.85 ms | **29.8× faster** |  |
| Random walks | 5,000 walks of length 20, deduplicated | 15.63 ms | 172.74 ms | **11.1× faster** | networkx has no random walks; pure Python over its adjacency |
| Serialize to JSON string | `save_to_json()` vs `node_link_data` + `json.dumps` | 237.59 ms | 295.96 ms | **1.2× faster** |  |
| Load from JSON string | `load_from_json()` vs `json.loads` + `node_link_graph` | 249.92 ms | 933.61 ms | **3.7× faster** |  |
