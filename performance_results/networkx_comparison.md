# ironweaver vs networkx

Generated 2026-09-26 09:33 by `benchmarks/compare_networkx.py`.

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
| Build graph | add all nodes and edges one by one | 4.45 ms | 11.32 ms | **2.5× faster** |  |
| BFS (full) | all nodes reachable from `n0` | 1.15 ms | 1.03 ms | 1.1× slower | 1,000 nodes reached |
| BFS (depth 3) | depth-limited BFS | 161.1 µs | 150.8 µs | 1.1× slower | 194 nodes reached |
| BFS (edge filter) | only follow `type == "knows"` edges | 1.09 ms | 5.55 ms | **5.1× faster** | 683 nodes reached |
| DFS traversal | pre-order DFS from source | 1.41 ms | 1.53 ms | **1.1× faster** |  |
| BFS search | find `n999` from `n0` | 45.6 µs | 82.8 µs | **1.8× faster** | both use bidirectional BFS |
| Shortest path (unweighted) | BFS shortest path | 72.7 µs | 89.6 µs | **1.2× faster** | 5 hops; both use bidirectional BFS |
| Shortest path (Dijkstra) | weighted by `weight` attribute | 1.89 ms | 3.35 ms | **1.8× faster** | cost 14.95 |
| Subgraph by ids | 200 nodes + edges between them, as a new graph | 534.4 µs | 2.52 ms | **4.7× faster** |  |
| Subgraph by attribute | nodes with `group == 3` | 456.8 µs | 907.9 µs | **2.0× faster** |  |
| Expand (depth 2) | 100 seeds + neighbourhood, as a new graph | 4.32 ms | 45.87 ms | **10.6× faster** | 966 nodes |
| Remove nodes | remove 50 nodes and their edges | 256.9 µs | 417.5 µs | **1.6× faster** |  |
| Count edges | `get_metadata()` vs `number_of_edges()` | 287.7 µs | 1.23 ms | **4.3× faster** |  |
| Random walks | 5,000 walks of length 20, deduplicated | 7.11 ms | 31.48 ms | **4.4× faster** | networkx has no random walks; pure Python over its adjacency |
| Serialize to JSON string | `save_to_json()` vs `node_link_data` + `json.dumps` | 9.46 ms | 11.73 ms | **1.2× faster** |  |
| Load from JSON string | `load_from_json()` vs `json.loads` + `node_link_graph` | 7.13 ms | 16.72 ms | **2.3× faster** |  |

## 20,000 nodes, 119,999 edges

| Operation | Description | ironweaver | networkx | Speedup | Notes |
|---|---|---:|---:|---:|---|
| Build graph | add all nodes and edges one by one | 306.26 ms | 576.91 ms | **1.9× faster** |  |
| BFS (full) | all nodes reachable from `n0` | 54.67 ms | 46.57 ms | 1.2× slower | 20,000 nodes reached |
| BFS (depth 3) | depth-limited BFS | 596.2 µs | 503.4 µs | 1.2× slower | 329 nodes reached |
| BFS (edge filter) | only follow `type == "knows"` edges | 66.6 µs | 2.78 ms | **41.7× faster** | 3 nodes reached |
| DFS traversal | pre-order DFS from source | 79.58 ms | 77.05 ms | 1.0× slower |  |
| BFS search | find `n19999` from `n0` | 245.7 µs | 318.6 µs | **1.3× faster** | both use bidirectional BFS |
| Shortest path (unweighted) | BFS shortest path | 252.5 µs | 329.0 µs | **1.3× faster** | 6 hops; both use bidirectional BFS |
| Shortest path (Dijkstra) | weighted by `weight` attribute | 31.07 ms | 56.00 ms | **1.8× faster** | cost 16.85 |
| Subgraph by ids | 4,000 nodes + edges between them, as a new graph | 21.70 ms | 68.64 ms | **3.2× faster** |  |
| Subgraph by attribute | nodes with `group == 3` | 12.47 ms | 19.23 ms | **1.5× faster** |  |
| Expand (depth 2) | 100 seeds + neighbourhood, as a new graph | 23.41 ms | 89.72 ms | **3.8× faster** | 3,950 nodes |
| Remove nodes | remove 1,000 nodes and their edges | 10.32 ms | 18.24 ms | **1.8× faster** |  |
| Count edges | `get_metadata()` vs `number_of_edges()` | 7.55 ms | 49.13 ms | **6.5× faster** |  |
| Random walks | 5,000 walks of length 20, deduplicated | 67.31 ms | 219.35 ms | **3.3× faster** | networkx has no random walks; pure Python over its adjacency |
| Serialize to JSON string | `save_to_json()` vs `node_link_data` + `json.dumps` | 260.78 ms | 384.03 ms | **1.5× faster** |  |
| Load from JSON string | `load_from_json()` vs `json.loads` + `node_link_graph` | 280.72 ms | 608.08 ms | **2.2× faster** |  |
