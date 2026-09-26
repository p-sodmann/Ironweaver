# ironweaver vs networkx

Generated 2026-09-26 08:40 by `benchmarks/compare_networkx.py`.

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
| Build graph | add all nodes and edges one by one | 4.22 ms | 10.75 ms | **2.6× faster** |  |
| BFS (full) | all nodes reachable from `n0` | 1.17 ms | 1.24 ms | **1.1× faster** | 1,000 nodes reached |
| BFS (depth 3) | depth-limited BFS | 229.4 µs | 181.2 µs | 1.3× slower | 194 nodes reached |
| BFS (edge filter) | only follow `type == "knows"` edges | 1.13 ms | 5.60 ms | **5.0× faster** | 683 nodes reached |
| DFS traversal | pre-order DFS from source | 1.43 ms | 1.40 ms | 1.0× slower |  |
| BFS search | find `n999` from `n0` | 415.4 µs | 85.6 µs | 4.9× slower | networkx `has_path` uses bidirectional BFS |
| Shortest path (unweighted) | BFS shortest path | 607.4 µs | 75.8 µs | 8.0× slower | 5 hops; networkx uses bidirectional BFS |
| Shortest path (Dijkstra) | weighted by `weight` attribute | 1.49 ms | 3.40 ms | **2.3× faster** | cost 14.95 |
| Subgraph by ids | 200 nodes + edges between them, as a new graph | 559.5 µs | 2.68 ms | **4.8× faster** |  |
| Subgraph by attribute | nodes with `group == 3` | 407.9 µs | 1.06 ms | **2.6× faster** |  |
| Expand (depth 2) | 100 seeds + neighbourhood, as a new graph | 4.72 ms | 41.40 ms | **8.8× faster** | 966 nodes |
| Remove nodes | remove 50 nodes and their edges | 230.4 µs | 451.6 µs | **2.0× faster** |  |
| Count edges | `get_metadata()` vs `number_of_edges()` | 228.3 µs | 1.10 ms | **4.8× faster** |  |
| Random walks | 5,000 walks of length 20, deduplicated | 8.02 ms | 30.01 ms | **3.7× faster** | networkx has no random walks; pure Python over its adjacency |
| Serialize to JSON string | `save_to_json()` vs `node_link_data` + `json.dumps` | 6.81 ms | 12.69 ms | **1.9× faster** |  |
| Load from JSON string | `load_from_json()` vs `json.loads` + `node_link_graph` | 7.67 ms | 17.83 ms | **2.3× faster** |  |

## 20,000 nodes, 119,999 edges

| Operation | Description | ironweaver | networkx | Speedup | Notes |
|---|---|---:|---:|---:|---|
| Build graph | add all nodes and edges one by one | 323.73 ms | 510.15 ms | **1.6× faster** |  |
| BFS (full) | all nodes reachable from `n0` | 63.94 ms | 46.95 ms | 1.4× slower | 20,000 nodes reached |
| BFS (depth 3) | depth-limited BFS | 563.6 µs | 417.0 µs | 1.4× slower | 329 nodes reached |
| BFS (edge filter) | only follow `type == "knows"` edges | 64.0 µs | 2.83 ms | **44.2× faster** | 3 nodes reached |
| DFS traversal | pre-order DFS from source | 79.38 ms | 73.41 ms | 1.1× slower |  |
| BFS search | find `n19999` from `n0` | 23.82 ms | 383.3 µs | 62.1× slower | networkx `has_path` uses bidirectional BFS |
| Shortest path (unweighted) | BFS shortest path | 30.30 ms | 361.2 µs | 83.9× slower | 6 hops; networkx uses bidirectional BFS |
| Shortest path (Dijkstra) | weighted by `weight` attribute | 37.03 ms | 57.75 ms | **1.6× faster** | cost 16.85 |
| Subgraph by ids | 4,000 nodes + edges between them, as a new graph | 22.54 ms | 63.40 ms | **2.8× faster** |  |
| Subgraph by attribute | nodes with `group == 3` | 13.45 ms | 21.73 ms | **1.6× faster** |  |
| Expand (depth 2) | 100 seeds + neighbourhood, as a new graph | 26.40 ms | 96.01 ms | **3.6× faster** | 3,950 nodes |
| Remove nodes | remove 1,000 nodes and their edges | 10.42 ms | 20.46 ms | **2.0× faster** |  |
| Count edges | `get_metadata()` vs `number_of_edges()` | 7.79 ms | 51.16 ms | **6.6× faster** |  |
| Random walks | 5,000 walks of length 20, deduplicated | 83.42 ms | 246.21 ms | **3.0× faster** | networkx has no random walks; pure Python over its adjacency |
| Serialize to JSON string | `save_to_json()` vs `node_link_data` + `json.dumps` | 266.55 ms | 363.64 ms | **1.4× faster** |  |
| Load from JSON string | `load_from_json()` vs `json.loads` + `node_link_graph` | 281.76 ms | 590.81 ms | **2.1× faster** |  |
