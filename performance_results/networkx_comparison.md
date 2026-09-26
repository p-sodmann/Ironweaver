# ironweaver vs networkx

Generated 2026-09-26 06:39 by `benchmarks/compare_networkx.py`.

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
| Build graph | add all nodes and edges one by one | 3.85 ms | 8.48 ms | **2.2× faster** |  |
| BFS (full) | all nodes reachable from `n0` | 909.2 µs | 1.11 ms | **1.2× faster** | 1,000 nodes reached |
| BFS (depth 3) | depth-limited BFS | 161.6 µs | 137.9 µs | 1.2× slower | 194 nodes reached |
| BFS (edge filter) | only follow `type == "knows"` edges | 1.07 ms | 5.40 ms | **5.0× faster** | 683 nodes reached |
| DFS traversal | pre-order DFS from source | 1.44 ms | 1.49 ms | **1.0× faster** |  |
| BFS search | find `n999` from `n0` | 389.1 µs | 81.0 µs | 4.8× slower | networkx `has_path` uses bidirectional BFS |
| Shortest path (unweighted) | BFS shortest path | 624.9 µs | 87.7 µs | 7.1× slower | 5 hops; networkx uses bidirectional BFS |
| Shortest path (Dijkstra) | weighted by `weight` attribute | 1.54 ms | 3.26 ms | **2.1× faster** | cost 14.95 |
| Subgraph by ids | 200 nodes + edges between them, as a new graph | 434.9 µs | 2.42 ms | **5.6× faster** |  |
| Subgraph by attribute | nodes with `group == 3` | 399.9 µs | 961.7 µs | **2.4× faster** |  |
| Expand (depth 2) | 100 seeds + neighbourhood, as a new graph | 4.88 ms | 40.71 ms | **8.3× faster** | 966 nodes |
| Remove nodes | remove 50 nodes and their edges | 295.2 µs | 931.2 µs | **3.2× faster** |  |
| Count edges | `get_metadata()` vs `number_of_edges()` | 195.0 µs | 1.14 ms | **5.8× faster** |  |
| Random walks | 5,000 walks of length 20, deduplicated | 6.66 ms | 31.82 ms | **4.8× faster** | networkx has no random walks; pure Python over its adjacency |
| Serialize to JSON string | `save_to_json()` vs `node_link_data` + `json.dumps` | 16.75 ms | 12.30 ms | 1.4× slower |  |
| Load from JSON string | `load_from_json()` vs `json.loads` + `node_link_graph` | 17.21 ms | 17.15 ms | 1.0× slower |  |

## 20,000 nodes, 119,999 edges

| Operation | Description | ironweaver | networkx | Speedup | Notes |
|---|---|---:|---:|---:|---|
| Build graph | add all nodes and edges one by one | 283.79 ms | 541.42 ms | **1.9× faster** |  |
| BFS (full) | all nodes reachable from `n0` | 43.33 ms | 31.58 ms | 1.4× slower | 20,000 nodes reached |
| BFS (depth 3) | depth-limited BFS | 445.3 µs | 375.5 µs | 1.2× slower | 329 nodes reached |
| BFS (edge filter) | only follow `type == "knows"` edges | 50.7 µs | 2.57 ms | **50.7× faster** | 3 nodes reached |
| DFS traversal | pre-order DFS from source | 65.55 ms | 55.81 ms | 1.2× slower |  |
| BFS search | find `n19999` from `n0` | 18.51 ms | 302.9 µs | 61.1× slower | networkx `has_path` uses bidirectional BFS |
| Shortest path (unweighted) | BFS shortest path | 26.72 ms | 273.8 µs | 97.6× slower | 6 hops; networkx uses bidirectional BFS |
| Shortest path (Dijkstra) | weighted by `weight` attribute | 28.14 ms | 44.75 ms | **1.6× faster** | cost 16.85 |
| Subgraph by ids | 4,000 nodes + edges between them, as a new graph | 14.83 ms | 51.02 ms | **3.4× faster** |  |
| Subgraph by attribute | nodes with `group == 3` | 11.38 ms | 19.05 ms | **1.7× faster** |  |
| Expand (depth 2) | 100 seeds + neighbourhood, as a new graph | 20.36 ms | 81.30 ms | **4.0× faster** | 3,950 nodes |
| Remove nodes | remove 1,000 nodes and their edges | 9.92 ms | 17.02 ms | **1.7× faster** |  |
| Count edges | `get_metadata()` vs `number_of_edges()` | 7.02 ms | 31.43 ms | **4.5× faster** |  |
| Random walks | 5,000 walks of length 20, deduplicated | 57.26 ms | 186.29 ms | **3.3× faster** | networkx has no random walks; pure Python over its adjacency |
| Serialize to JSON string | `save_to_json()` vs `node_link_data` + `json.dumps` | 761.52 ms | 259.89 ms | 2.9× slower |  |
| Load from JSON string | `load_from_json()` vs `json.loads` + `node_link_graph` | 676.89 ms | 631.19 ms | 1.1× slower |  |
