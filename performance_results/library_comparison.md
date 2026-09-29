# Graph library comparison

Generated 2026-09-29 by `benchmarks/compare_libraries.py` on Linux x86_64, 4 CPUs, Python 3.11.15.
Versions: ironweaver 0.1.0, networkx 3.6.1, igraph 1.0.0, rustworkx 0.18.1, networkit 11.2.2.

Best of the runs; the fastest per row in bold. Check: `=` same result as ironweaver, `≈` within
the stated difference, `✗` a different result. Communities are different algorithms, compared by
modularity Q (higher is better). "ironweaver" includes building the projection each time;
"ironweaver (reused)" runs on one built beforehand. Exact betweenness runs only where
nodes × edges ≤ 5e9; networkx's only with `--networkx-betweenness`.

### facebook: Facebook pages (SNAP musae-facebook)

22,470 nodes, 170,823 undirected edges.

| Operation | ironweaver | ironweaver (reused) | networkx | igraph | rustworkx | networkit | Check |
|---|---|---|---|---|---|---|---|
| Build graph from an edge list | 173.8 ms | – | 279.1 ms | 46.7 ms | 162.7 ms | 59.6 ms | |
| Weakly connected components | 14.6 ms | 3.7 ms | 34.0 ms | **2.2 ms** | 13.6 ms | 5.5 ms | networkx =; igraph =; rustworkx =; networkit = |
| BFS levels from one node | 17.1 ms | 7.2 ms | 34.8 ms | 8.1 ms | 11.4 ms | **6.4 ms** | networkx =; igraph =; rustworkx =; networkit = |
| Dijkstra from one node (weighted) | 31.6 ms | 20.8 ms | 278.9 ms | 24.7 ms | 39.1 ms | **13.3 ms** | networkx =; igraph =; rustworkx =; networkit = |
| PageRank | 45.0 ms | 28.5 ms | 399.7 ms | **27.4 ms** | – | 36.3 ms | networkx ≈ (max diff 4e-19); igraph ≈ (max diff 1e-07); networkit ≈ (max diff 1e-07) |
| Core number | 21.3 ms | 9.9 ms | 204.4 ms | 9.6 ms | 47.4 ms | **7.9 ms** | networkx =; igraph =; rustworkx =; networkit = |
| Local clustering | 58.7 ms | 46.0 ms | 2.62 s | 24.1 ms | – | **21.7 ms** | networkx =; igraph ≈ (max diff 1e-16); networkit = |
| Betweenness (exact) | **24.58 s** | 25.08 s | – | 98.93 s | 75.71 s | 67.90 s | igraph ≈ (max diff 3e-16); rustworkx ≈ (max diff 4e-16); networkit ≈ (max diff 5e-16) |
| Communities (Leiden / Louvain) | 245.8 ms | 247.6 ms | 3.18 s | 830.5 ms | – | **62.5 ms** | ironweaver: Q=0.818, 68 groups; networkx: Q=0.815, 64 groups; igraph: Q=0.819, 69 groups; networkit: Q=0.818, 63 groups |
| Minimum spanning forest (weight) | 35.4 ms | 23.5 ms | 508.8 ms | 58.1 ms | **14.0 ms** | 15.0 ms | networkx =; igraph ≈ (diff 7e-10); rustworkx =; networkit = |

### github: GitHub developers (SNAP musae-github)

37,700 nodes, 289,003 undirected edges.

| Operation | ironweaver | ironweaver (reused) | networkx | igraph | rustworkx | networkit | Check |
|---|---|---|---|---|---|---|---|
| Build graph from an edge list | 395.1 ms | – | 706.2 ms | 63.6 ms | 176.9 ms | 78.7 ms | |
| Weakly connected components | 29.5 ms | 7.0 ms | 68.9 ms | **4.1 ms** | 28.2 ms | 8.7 ms | networkx =; igraph =; rustworkx =; networkit = |
| BFS levels from one node | 32.7 ms | 11.2 ms | 64.1 ms | 13.1 ms | 20.3 ms | **10.4 ms** | networkx =; igraph =; rustworkx =; networkit = |
| Dijkstra from one node (weighted) | 62.5 ms | 23.9 ms | 588.0 ms | 40.3 ms | 77.5 ms | **21.6 ms** | networkx =; igraph =; rustworkx =; networkit = |
| PageRank | 50.6 ms | **25.9 ms** | 801.2 ms | 51.1 ms | – | 37.1 ms | networkx ≈ (max diff 3e-18); igraph ≈ (max diff 5e-08); networkit ≈ (max diff 5e-08) |
| Core number | 37.5 ms | 15.6 ms | 1.17 s | 16.5 ms | 105.0 ms | **10.3 ms** | networkx =; igraph =; rustworkx =; networkit = |
| Local clustering | 263.5 ms | 226.4 ms | 23.39 s | **36.2 ms** | – | 122.6 ms | networkx =; igraph ≈ (max diff 1e-16); networkit = |
| Communities (Leiden / Louvain) | 774.2 ms | 859.8 ms | 7.50 s | 3.38 s | – | **118.7 ms** | ironweaver: Q=0.460, 45 groups; networkx: Q=0.452, 34 groups; igraph: Q=0.465, 36 groups; networkit: Q=0.459, 37 groups |
| Minimum spanning forest (weight) | 73.7 ms | 31.5 ms | 1.22 s | 107.6 ms | **22.5 ms** | 25.8 ms | networkx =; igraph ≈ (diff 4e-09); rustworkx =; networkit = |

### kron: Kronecker scale 16, edge factor 16 (Graph500 R-MAT)

65,536 nodes, 955,025 directed edges.

| Operation | ironweaver | ironweaver (reused) | networkx | igraph | rustworkx | networkit | Check |
|---|---|---|---|---|---|---|---|
| Build graph from an edge list | 1.67 s | – | 3.09 s | 306.0 ms | 302.4 ms | 457.5 ms | |
| Weakly connected components | 93.4 ms | 33.3 ms | 169.5 ms | 61.8 ms | 215.4 ms | **33.0 ms** | networkx =; igraph =; rustworkx =; networkit = |
| Strongly connected components | 101.8 ms | **29.7 ms** | 700.8 ms | 73.1 ms | 219.0 ms | 38.6 ms | networkx =; igraph =; rustworkx =; networkit = |
| BFS levels from one node | 85.8 ms | **12.4 ms** | 70.2 ms | 24.5 ms | 91.5 ms | 15.5 ms | networkx =; igraph =; rustworkx =; networkit = |
| Dijkstra from one node (weighted) | 159.6 ms | **32.7 ms** | 1.11 s | 65.2 ms | 259.1 ms | 33.9 ms | networkx =; igraph =; rustworkx =; networkit = |
| PageRank | 116.2 ms | **32.7 ms** | 4.70 s | 94.7 ms | 585.6 ms | 42.8 ms | networkx ≈ (max diff 2e-16); igraph ≈ (max diff 2e-09); rustworkx ≈ (max diff 5e-09); networkit ≈ (max diff 2e-09) |
| Core number | 114.3 ms | 44.5 ms | 4.29 s | 444.9 ms | 24.05 s | **33.1 ms** | networkx =; igraph =; rustworkx =; networkit = |
| Local clustering | 1.65 s | 1.59 s | 114.40 s | 890.0 ms | – | **698.2 ms** | networkx =; igraph ≈ (max diff 1e-16); networkit = |
| Communities (Leiden / Louvain) | 3.28 s | 3.05 s | 24.29 s | 34.12 s | – | **321.3 ms** | ironweaver: Q=0.094, 18,778 groups; networkx: Q=0.094, 18,772 groups; igraph: Q=0.100, 18,773 groups; networkit: Q=0.099, 18,774 groups |
| Minimum spanning forest (weight) | 256.9 ms | 134.7 ms | 13.79 s | 1.08 s | 117.1 ms | **68.4 ms** | networkx =; igraph ≈ (diff 6e-09); rustworkx =; networkit = |
