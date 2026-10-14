# Search issues: validation and benchmarks

Addresses #118–#121. Base: `306b832` (`main`); after: the accompanying PR.
Python 3.12.14, Rust 1.99.0, Linux cloud executor, release builds,
`RAYON_NUM_THREADS=1`. The same dependency versions were used for both builds.

## BFS

`benchmarks/compare_bfs.py --repeats 15`: 10,000 nodes, 100 queries per
workload, seed 120. Grid: 100 × 100, directed edges both ways; random graph:
a directed cycle plus three random outgoing edges per node. Grid trips
include both random pairs and all four opposite-corner routes. Times are
medians of three runs, each itself the median of 15 repetitions. Runs
alternate base/PR order. Negative changes mean faster.

| Workload | Query | Before (ms) | After (ms) | Change |
|---|---|---:|---:|---:|
| grid random trips | shortest_path | 0.2235 | 0.2302 | +3.0% |
| grid random trips | bfs_search | 0.1881 | 0.1543 | -18.0% |
| grid corner trips | shortest_path | 0.5006 | 0.5033 | +0.5% |
| grid corner trips | bfs_search | 0.4315 | 0.4164 | -3.5% |
| random | shortest_path | 0.0234 | 0.0272 | +16.2% |
| random | bfs_search | 0.0108 | 0.0078 | -27.8% |

Reachability avoids canonical reconstruction entirely. Path queries retain
the canonical rule, with target-side viability checked lazily and cached;
source-side reconstruction remains one linear pass per path node. Scratch
storage is reused along the path, and unweighted BFS has a separate
specialization without per-edge weight validation.

The shared executor has timing variation; small differences in complete
`shortest_path` calls are not evidence of a stable speedup or regression.
The clearest improvement is sparse-graph reachability. Full raw runs are
in [benchmark-rounds.txt](benchmark-rounds.txt).

## A* and default statistics overhead

`benchmarks/compare_astar.py --side 200 --trips 100 --repeats 9`: 39,001
nodes, 151,320 edges; 2.5% obstacles, 20% rough tiles, seed 0. Medians of
three alternating runs, each itself the median of nine repetitions.
`stats` is not requested in these timings; the core batch search loops are
unchanged and the existing counters are converted only when requested.

| Workload | Query | Before (ms) | After (ms) | Change |
|---|---|---:|---:|---:|
| dijkstra | single | 0.8640 | 0.9180 | +6.2% |
| dijkstra | batch | 0.8710 | 0.8470 | -2.8% |
| astar manhattan | single | 0.2070 | 0.2060 | -0.5% |
| astar manhattan | batch | 0.1590 | 0.1550 | -2.5% |
| astar euclidean | single | 0.4700 | 0.4710 | +0.2% |
| astar euclidean | batch | 0.4320 | 0.4180 | -3.2% |

Default batch timings remain within a few percent of the base, against
larger run-to-run variation. Single-call timing is noisier, including a
small increase for Manhattan A*; these measurements do not establish a
search-loop regression. Costs matched for every method and trip.

With `--stats`, median settled counts were 18,512 for Dijkstra, 692 for
Manhattan A*, and 3,954 for Euclidean A*. See
[astar-stats.txt](astar-stats.txt). The API deliberately exposes `settled`
only, reusing the core counters without adding heap/edge bookkeeping.
Dijkstra counts settled nodes, A* counts expansions including reopens,
batch BFS counts reached nodes, and bidirectional BFS counts expanded
frontier nodes from both sides. Canonical reconstruction is excluded.
Unreachable batch entries remain `None`.

## Identity and passability regressions

The source identity is a retained `Arc<()>` token, independent of the
Vertex's address. It keeps no graph or Python Vertex alive. Keeping the
token allocated prevents identity reuse for the projection's lifetime;
`Vertex.__clear__` replaces it when the graph is reset. Comparison is O(1).
The CPython regression freed the source and reused its address on the
first replacement allocation, with the same edge slot. `edge_index`
correctly rejected the unrelated edge.

Weighted BFS was compared with weighted projections on 36 seeded random
graphs and 720 queries across out/in/both directions, including infinity,
zero, missing weights, parallel edges and self-loops. Paths and hop costs
matched, including canonical ties. Unweighted `Node.bfs_search` remains
unchanged in its weight semantics and now stops at the first meeting.

## Checks

- `pytest -q`: 1,173 passed, including documentation examples and Python tie breaks.
- `cargo test -p ironweaver-core`: passed, including core tie breaks, budgets,
  randomized reference comparisons, and the new counter checks.
- `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `RUSTDOCFLAGS="-D warnings" cargo doc -p ironweaver-core --no-deps`: passed.
- Core dependency tree contains no PyO3.
