# ironweaver vs networkx

Generated 2026-09-29 09:14 by `benchmarks/compare_networkx.py`.

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
| Build graph | add all nodes and edges one by one | 5.61 ms | 11.92 ms | **2.1× faster** |  |
| BFS (full) | all nodes reachable from `n0` | 1.01 ms | 866.7 µs | 1.2× slower | 1,000 nodes reached |
| BFS (depth 3) | depth-limited BFS | 163.1 µs | 120.5 µs | 1.4× slower | 194 nodes reached |
| BFS (edge filter) | only follow `type == "knows"` edges | 844.4 µs | 6.49 ms | **7.7× faster** | 683 nodes reached |
| DFS traversal | pre-order DFS from source | 1.37 ms | 1.39 ms | **1.0× faster** |  |
| BFS search | find `n999` from `n0` | 29.7 µs | 73.6 µs | **2.5× faster** | both use bidirectional BFS |
| Shortest path (unweighted) | BFS shortest path | 54.1 µs | 61.0 µs | **1.1× faster** | 5 hops; both use bidirectional BFS |
| Shortest path (Dijkstra) | weighted by `weight` attribute | 904.7 µs | 3.35 ms | **3.7× faster** | cost 14.95 |
| Batch shortest paths | 64 Dijkstra pairs in one `shortest_paths` call | 2.12 ms | 212.51 ms | **100.2× faster** | one `shortest_path` call per pair: 26.21 ms |
| Distances from sources | Dijkstra from 16 sources to every reachable node | 2.89 ms | 91.87 ms | **31.8× faster** | 16,000 distances |
| Project graph | `project(weight=...)`: compact read-only copy for analytics | 527.9 µs | – | – | 0.1 MB |
| Weakly connected components | `project().weakly_connected_components()` | 351.2 µs | 746.8 µs | **2.1× faster** | 1 components |
| Strongly connected components | `project().strongly_connected_components()` | 355.0 µs | 1.50 ms | **4.2× faster** | 1 components |
| PageRank | weighted, `project(weight=...).pagerank()` | 1.91 ms | 6.82 ms | **3.6× faster** |  |
| Triangles | per node, edges as undirected (networkx: incl. `nx.Graph` conversion) | 1.44 ms | 45.44 ms | **31.6× faster** | 279 triangles |
| Core number | k-core decomposition (networkx: incl. `nx.Graph` conversion) | 960.6 µs | 36.93 ms | **38.4× faster** | max core 8 |
| Label propagation | communities; networkx uses its semi-synchronous variant | 2.21 ms | 54.86 ms | **24.8× faster** | 1 vs 1 communities |
| BFS levels | hop distance from one node to all (parallel, direction-optimizing) | 1.24 ms | 792.6 µs | 1.6× slower | 1,000 reached |
| Grid: Dijkstra | 31×31 grid, left edge to right edge | 550.0 µs | 2.78 ms | **5.1× faster** | 721 nodes expanded |
| Grid: A* (coordinates) | Euclidean heuristic from `x`/`y` node attributes | 134.2 µs | 385.8 µs | **2.9× faster** | 51 nodes expanded; networkx calls a Python heuristic |
| Grid: A* (precomputed) | estimates from `vertex.meta` vs a dict lookup | 153.8 µs | 274.4 µs | **1.8× faster** | 51 nodes expanded |
| Subgraph by ids | 200 nodes + edges between them, as a new graph | 263.9 µs | 2.88 ms | **10.9× faster** |  |
| Subgraph by attribute | nodes with `group == 3` | 145.2 µs | 1.09 ms | **7.5× faster** |  |
| Expand (depth 2) | 100 seeds + neighbourhood, as a new graph | 1.19 ms | 46.60 ms | **39.1× faster** | 966 nodes |
| Remove nodes | remove 50 nodes and their edges | 245.7 µs | 363.5 µs | **1.5× faster** |  |
| Count edges | `get_metadata()` vs `number_of_edges()` | 79.1 µs | 1.42 ms | **18.0× faster** |  |
| Random walks | 5,000 walks of length 20, deduplicated | 7.43 ms | 43.03 ms | **5.8× faster** | networkx has no random walks; pure Python over its adjacency |
| Serialize to JSON string | `save_to_json()` vs `node_link_data` + `json.dumps` | 4.03 ms | 9.69 ms | **2.4× faster** |  |
| Load from JSON string | `load_from_json()` vs `json.loads` + `node_link_graph` | 7.52 ms | 22.43 ms | **3.0× faster** |  |

## 20,000 nodes, 119,999 edges

| Operation | Description | ironweaver | networkx | Speedup | Notes |
|---|---|---:|---:|---:|---|
| Build graph | add all nodes and edges one by one | 168.45 ms | 495.23 ms | **2.9× faster** |  |
| BFS (full) | all nodes reachable from `n0` | 57.20 ms | 39.00 ms | 1.5× slower | 20,000 nodes reached |
| BFS (depth 3) | depth-limited BFS | 517.7 µs | 377.6 µs | 1.4× slower | 329 nodes reached |
| BFS (edge filter) | only follow `type == "knows"` edges | 116.3 µs | 2.72 ms | **23.4× faster** | 3 nodes reached |
| DFS traversal | pre-order DFS from source | 56.49 ms | 52.10 ms | 1.1× slower |  |
| BFS search | find `n19999` from `n0` | 158.8 µs | 365.0 µs | **2.3× faster** | both use bidirectional BFS |
| Shortest path (unweighted) | BFS shortest path | 245.0 µs | 280.2 µs | **1.1× faster** | 6 hops; both use bidirectional BFS |
| Shortest path (Dijkstra) | weighted by `weight` attribute | 13.10 ms | 50.33 ms | **3.8× faster** | cost 16.85 |
| Batch shortest paths | 64 Dijkstra pairs in one `shortest_paths` call | 47.29 ms | 7.989 s | **169.0× faster** | one `shortest_path` call per pair: 1.281 s |
| Distances from sources | Dijkstra from 16 sources to every reachable node | 91.81 ms | 4.436 s | **48.3× faster** | 320,000 distances |
| Project graph | `project(weight=...)`: compact read-only copy for analytics | 12.48 ms | – | – | 2.3 MB |
| Weakly connected components | `project().weakly_connected_components()` | 9.08 ms | 37.72 ms | **4.2× faster** | 1 components |
| Strongly connected components | `project().strongly_connected_components()` | 9.61 ms | 61.34 ms | **6.4× faster** | 1 components |
| PageRank | weighted, `project(weight=...).pagerank()` | 19.59 ms | 221.69 ms | **11.3× faster** |  |
| Triangles | per node, edges as undirected (networkx: incl. `nx.Graph` conversion) | 23.61 ms | 1.149 s | **48.7× faster** | 247 triangles |
| Core number | k-core decomposition (networkx: incl. `nx.Graph` conversion) | 13.53 ms | 1.116 s | **82.4× faster** | max core 8 |
| Label propagation | communities; networkx uses its semi-synchronous variant | 18.68 ms | 2.142 s | **114.6× faster** | 1 vs 1 communities |
| BFS levels | hop distance from one node to all (parallel, direction-optimizing) | 13.88 ms | 33.48 ms | **2.4× faster** | 20,000 reached |
| Grid: Dijkstra | 141×141 grid, left edge to right edge | 20.43 ms | 98.17 ms | **4.8× faster** | 15,186 nodes expanded |
| Grid: A* (coordinates) | Euclidean heuristic from `x`/`y` node attributes | 2.27 ms | 8.48 ms | **3.7× faster** | 987 nodes expanded; networkx calls a Python heuristic |
| Grid: A* (precomputed) | estimates from `vertex.meta` vs a dict lookup | 2.28 ms | 6.57 ms | **2.9× faster** | 987 nodes expanded |
| Subgraph by ids | 4,000 nodes + edges between them, as a new graph | 7.24 ms | 85.09 ms | **11.8× faster** |  |
| Subgraph by attribute | nodes with `group == 3` | 3.33 ms | 21.75 ms | **6.5× faster** |  |
| Expand (depth 2) | 100 seeds + neighbourhood, as a new graph | 7.10 ms | 94.71 ms | **13.3× faster** | 3,950 nodes |
| Remove nodes | remove 1,000 nodes and their edges | 9.89 ms | 15.73 ms | **1.6× faster** |  |
| Count edges | `get_metadata()` vs `number_of_edges()` | 1.66 ms | 50.78 ms | **30.6× faster** |  |
| Random walks | 5,000 walks of length 20, deduplicated | 18.42 ms | 193.23 ms | **10.5× faster** | networkx has no random walks; pure Python over its adjacency |
| Serialize to JSON string | `save_to_json()` vs `node_link_data` + `json.dumps` | 178.39 ms | 342.45 ms | **1.9× faster** |  |
| Load from JSON string | `load_from_json()` vs `json.loads` + `node_link_graph` | 264.10 ms | 595.66 ms | **2.3× faster** |  |
