# Benchmarks

`benchmarks/compare_libraries.py` runs common algorithms with ironweaver, networkx, igraph, rustworkx and networkit on medium-sized graphs, and checks that every library's result agrees with ironweaver's. `tests/test_graphalytics.py` checks ironweaver against the reference outputs of the [LDBC Graphalytics](https://ldbcouncil.org/benchmarks/graphalytics/) benchmark.

--8<-- "performance_results/library_comparison.md"
