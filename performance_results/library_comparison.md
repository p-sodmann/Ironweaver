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
| Build graph from an edge list | 127.5 ms | – | 235.7 ms | 20.3 ms | 27.1 ms | 49.0 ms | |
| Weakly connected components | 12.7 ms | 3.4 ms | 30.5 ms | **2.3 ms** | 11.3 ms | 4.9 ms | networkx =; igraph =; rustworkx =; networkit = |
| BFS levels from one node | 15.1 ms | 6.8 ms | 38.0 ms | 8.7 ms | 8.9 ms | **6.0 ms** | networkx =; igraph =; rustworkx =; networkit = |
| Dijkstra from one node (weighted) | 27.6 ms | 17.3 ms | 286.0 ms | 18.7 ms | 36.3 ms | **16.3 ms** | networkx =; igraph =; rustworkx =; networkit = |
| PageRank | 40.0 ms | **27.6 ms** | 233.5 ms | 31.4 ms | – | 37.8 ms | networkx ≈ (max diff 4e-19); igraph ≈ (max diff 1e-07); networkit ≈ (max diff 1e-07) |
| Core number | 24.9 ms | 9.2 ms | 288.8 ms | **8.2 ms** | 52.8 ms | 9.2 ms | networkx =; igraph =; rustworkx =; networkit = |
| Local clustering | 25.9 ms | **18.1 ms** | 2.73 s | 26.3 ms | – | 23.5 ms | networkx =; igraph ≈ (max diff 1e-16); networkit = |
| Betweenness (exact) | 25.82 s | **24.78 s** | – | 101.67 s | 76.35 s | 73.34 s | igraph ≈ (max diff 3e-16); rustworkx ≈ (max diff 5e-16); networkit ≈ (max diff 4e-16) |
| Communities (Leiden / Louvain) | 103.4 ms | 97.3 ms | 3.04 s | 831.7 ms | – | **65.2 ms** | ironweaver: Q=0.818, 69 groups; networkx: Q=0.815, 64 groups; igraph: Q=0.819, 69 groups; networkit: Q=0.818, 65 groups |
| Minimum spanning forest (weight) | 34.6 ms | 19.9 ms | 382.7 ms | 48.0 ms | 14.3 ms | **13.8 ms** | networkx =; igraph ≈ (diff 7e-10); rustworkx =; networkit = |

### github: GitHub developers (SNAP musae-github)

37,700 nodes, 289,003 undirected edges.

| Operation | ironweaver | ironweaver (reused) | networkx | igraph | rustworkx | networkit | Check |
|---|---|---|---|---|---|---|---|
| Build graph from an edge list | 326.3 ms | – | 426.1 ms | 39.4 ms | 42.6 ms | 64.8 ms | |
| Weakly connected components | 25.5 ms | 7.1 ms | 65.5 ms | **4.1 ms** | 25.8 ms | 8.8 ms | networkx =; igraph =; rustworkx =; networkit = |
| BFS levels from one node | 23.4 ms | **10.5 ms** | 63.0 ms | 15.3 ms | 22.9 ms | 12.2 ms | networkx =; igraph =; rustworkx =; networkit = |
| Dijkstra from one node (weighted) | 44.6 ms | 25.0 ms | 573.1 ms | 38.0 ms | 70.6 ms | **20.8 ms** | networkx =; igraph =; rustworkx =; networkit = |
| PageRank | 43.2 ms | **23.1 ms** | 450.3 ms | 54.3 ms | – | 38.8 ms | networkx ≈ (max diff 3e-18); igraph ≈ (max diff 5e-08); networkit ≈ (max diff 5e-08) |
| Core number | 28.9 ms | 16.5 ms | 1.30 s | 14.8 ms | 104.9 ms | **12.0 ms** | networkx =; igraph =; rustworkx =; networkit = |
| Local clustering | 36.2 ms | **21.9 ms** | 23.40 s | 40.3 ms | – | 126.2 ms | networkx =; igraph ≈ (max diff 1e-16); networkit = |
| Communities (Leiden / Louvain) | 222.9 ms | 195.2 ms | 7.03 s | 3.28 s | – | **133.5 ms** | ironweaver: Q=0.459, 41 groups; networkx: Q=0.452, 34 groups; igraph: Q=0.465, 36 groups; networkit: Q=0.457, 35 groups |
| Minimum spanning forest (weight) | 59.9 ms | 32.6 ms | 839.5 ms | 104.8 ms | **23.8 ms** | 28.8 ms | networkx =; igraph ≈ (diff 4e-09); rustworkx =; networkit = |

### kron: Kronecker scale 16, edge factor 16 (Graph500 R-MAT)

65,536 nodes, 955,025 directed edges.

| Operation | ironweaver | ironweaver (reused) | networkx | igraph | rustworkx | networkit | Check |
|---|---|---|---|---|---|---|---|
| Build graph from an edge list | 1.25 s | – | 2.55 s | 363.0 ms | 388.3 ms | 470.3 ms | |
| Weakly connected components | 64.6 ms | **26.2 ms** | 166.8 ms | 50.6 ms | 166.7 ms | 31.3 ms | networkx =; igraph =; rustworkx =; networkit = |
| Strongly connected components | 69.4 ms | **28.3 ms** | 538.6 ms | 61.1 ms | 182.1 ms | 32.2 ms | networkx =; igraph =; rustworkx =; networkit = |
| BFS levels from one node | 51.7 ms | **11.1 ms** | 81.5 ms | 24.0 ms | 106.5 ms | 16.1 ms | networkx =; igraph =; rustworkx =; networkit = |
| Dijkstra from one node (weighted) | 101.3 ms | **29.2 ms** | 1.13 s | 74.8 ms | 260.0 ms | 33.4 ms | networkx =; igraph =; rustworkx =; networkit = |
| PageRank | 74.8 ms | **30.5 ms** | 1.44 s | 94.8 ms | 420.8 ms | 42.8 ms | networkx ≈ (max diff 2e-16); igraph ≈ (max diff 2e-09); rustworkx ≈ (max diff 5e-09); networkit ≈ (max diff 2e-09) |
| Core number | 91.1 ms | 45.1 ms | 4.30 s | 490.7 ms | 23.38 s | **35.0 ms** | networkx =; igraph =; rustworkx =; networkit = |
| Local clustering | 158.6 ms | **107.9 ms** | 109.31 s | 803.9 ms | – | 681.2 ms | networkx =; igraph ≈ (max diff 1e-16); networkit = |
| Communities (Leiden / Louvain) | 631.9 ms | 573.7 ms | 21.02 s | 35.12 s | – | **341.0 ms** | ironweaver: Q=0.100, 18,769 groups; networkx: Q=0.094, 18,772 groups; igraph: Q=0.100, 18,773 groups; networkit: Q=0.099, 18,771 groups |
| Minimum spanning forest (weight) | 209.5 ms | 123.1 ms | 7.17 s | 1.05 s | 131.9 ms | **65.3 ms** | networkx =; igraph ≈ (diff 6e-09); rustworkx =; networkit = |
