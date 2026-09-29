# LDBC Graphalytics validation graphs

The validation graphs and reference outputs of the LDBC Graphalytics
benchmark (BFS, CDLP, LCC, PR, SSSP, WCC), copied unchanged from
`graphalytics-validation/src/main/resources/validation-graphs` of
https://github.com/ldbc/ldbc_graphalytics (commit 7b8bde7, December 2024).
Licensed under the Apache License 2.0 (see `LICENSE`). The `ffm` directory
(inputs for the forest-fire graph generator, without outputs) is left out.

`tests/test_graphalytics.py` runs every case with the parameters of the
Graphalytics validation tests.

Formats:
- `*-input` (and `example-*-input`): one line per vertex, the vertex id
  followed by its out-neighbours (undirected graphs list both directions).
- `*.v` / `*.e`: vertex ids; edges `source target weight`.
- outputs: `vertex value` per line; BFS marks unreachable vertices with
  9223372036854775807, SSSP with `Infinity`.
