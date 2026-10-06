# Same-machine full revision benchmark

This report compares baseline `7e7b7fa40e3224b512d59d046f28b5bc0ffe7144`
with the revised node-deletion implementation in this PR. Both artifacts were
freshly built and measured on the **same VM**. Historical reports and timings
from other machines are not baselines.

All 143 applicable metrics from the three existing Python benchmark scripts
were measured, followed by all 17 focused deletion fixtures. Five slower screen
metrics received independent 20-pair repeats; similarity received a further
60-pair repeat with 25 samples/process and another 60-pair repeat with the
original seven-sample confirmation protocol. Nine deletion controls were
repeated with larger batches.
All raw measurements, including unfavorable and inconclusive runs, are retained.

Parallel-edge deletion improves decisively. The broad suite exposes an
**unresolved similarity timing difference**, so the PR remains a draft.
Short-run similarity repeats lean slower, while a longer-sample repeat is equal.
No independent interval establishes a loss greater than 5%, but several
intervals include moderate losses; this does not establish equivalence or a
regression-free change. Unrelated algorithm fluctuations are not claimed as
optimization gains.

## Protocol and provenance

- Linux x86-64 VM, Intel Xeon Platinum 8370C, five available CPUs; Python
  3.12.14 and Rust 1.99.0. Same standard release flags, copied Cargo.lock,
  interpreter, Python dependencies, CPU affinity and unchanged binding sources.
- Broad suite: six independent process pairs, alternating AB/BA and reversing
  case order. One warmup and three measured samples per operation, summarized
  by their median. Exact betweenness preserves the original one-run rule and
  budget; it is not dropped just because it is expensive.
- Operations are interleaved behind barriers: the counterpart waits during
  both timing and preparation. No builds, tests or other benchmark jobs ran
  concurrently. Python hash seed 42, Rayon five threads, BLAS/OpenMP one thread.
- Memory retains fresh-process workers, node-count checks and working Linux
  peak-RSS resets. Resident graph memory, JSON peaks and both file sizes run
  for both default sizes.
- NetworkX reference assertions still execute. Untimed NetworkX oracles are
  locally cached before measurement; Ironweaver results/timings are never cached.
  Unrelated competing libraries are not repeated in a revision comparison.
- Paired geometric-mean baseline/candidate ratios and seeded 10,000-resample
  95% bootstrap intervals resample independent process pairs, not inner samples.
  A screening flag means the interval upper bound is below `1/1.05`.
  Screening 143 metrics needs independent confirmation; these are nominal,
  per-metric intervals, not simultaneous confidence guarantees.
- Workers all recorded one identical machine/runtime identity and one constant
  extension hash per revision. The measured candidate core-source hash matches
  this PR's `graph.rs`. Binding hashes are identical between builds.

Lockfile SHA-256: `8a88c8fe93618bb1741941a80608c7a75c627ed2fe60d2b70b25d7cd6272623a`.

Candidate `graph.rs` SHA-256: `f20d222c01bf1df7fa19e3d8b44d21ee770392e0ce2f837f8dd4abc99286470e`.

## Complete broad screen

Before/after timings are seconds; memory/file sizes are bytes. Ratio >1 favors
the candidate. A blank flag means the screen did not establish a >5% slowdown;
it is not a claim of equality. These tables include every applicable workload.

### networkx:1000:5000

| Workload | Before | After | Paired ratio | 95% interval | Screen flag |
|---|---:|---:|---:|---:|---|
| Build graph | 0.006520692 | 0.006785833 | 0.896 | 0.680–1.122 |  |
| BFS (full) | 0.001746269 | 0.001927972 | 0.853 | 0.678–1.069 |  |
| BFS (depth 3) | 0.000287269 | 0.000274693 | 1.246 | 0.950–1.915 |  |
| BFS (edge filter) | 0.001414403 | 0.001919791 | 0.700 | 0.389–1.188 |  |
| DFS traversal | 0.001916658 | 0.002207672 | 0.869 | 0.470–1.422 |  |
| BFS search | 0.000049983 | 0.000050539 | 1.013 | 0.916–1.125 |  |
| Shortest path (unweighted) | 0.000081687 | 0.000082490 | 0.920 | 0.533–1.448 |  |
| Shortest path (Dijkstra) | 0.001488093 | 0.001568062 | 0.949 | 0.774–1.120 |  |
| Batch shortest paths | 0.003498056 | 0.003312150 | 0.853 | 0.614–1.185 |  |
| Distances from sources | 0.003869547 | 0.003522530 | 1.189 | 0.918–1.563 |  |
| Project graph | 0.000622323 | 0.000583663 | 1.096 | 0.897–1.431 |  |
| Weakly connected components | 0.000347566 | 0.000353715 | 1.193 | 0.948–1.671 |  |
| Strongly connected components | 0.000390432 | 0.000468564 | 0.852 | 0.700–1.021 |  |
| PageRank | 0.004018746 | 0.003730532 | 0.951 | 0.707–1.241 |  |
| Triangles | 0.001514790 | 0.001694516 | 0.839 | 0.647–1.068 |  |
| Core number | 0.001066179 | 0.001144328 | 1.032 | 0.778–1.401 |  |
| Label propagation | 0.004075240 | 0.003789675 | 1.150 | 0.968–1.408 |  |
| BFS levels | 0.001716752 | 0.001390612 | 0.951 | 0.645–1.288 |  |
| Betweenness (sampled) | 0.003107940 | 0.003808297 | 0.951 | 0.812–1.212 |  |
| Similarity | 0.006931151 | 0.006180489 | 1.079 | 0.904–1.253 |  |
| Most similar nodes | 0.004076421 | 0.004436914 | 0.880 | 0.780–0.985 |  |
| Leiden vs Louvain | 0.013058123 | 0.012230718 | 1.061 | 0.927–1.205 |  |
| Minimum spanning tree | 0.001274861 | 0.001449233 | 0.862 | 0.681–1.072 |  |
| k shortest paths | 0.021292734 | 0.021234816 | 0.975 | 0.913–1.031 |  |
| FastRP embeddings | 0.005373324 | 0.005470995 | 0.880 | 0.711–1.058 |  |
| node2vec walks | 0.002592027 | 0.002694186 | 0.956 | 0.864–1.037 |  |
| Grid: Dijkstra | 0.000823285 | 0.000838709 | 1.142 | 0.658–2.396 |  |
| Grid: A* (coordinates) | 0.000186843 | 0.000185691 | 1.031 | 0.838–1.318 |  |
| Grid: A* (precomputed) | 0.000198446 | 0.000176542 | 1.068 | 0.966–1.168 |  |
| Subgraph by ids | 0.000387580 | 0.000327123 | 1.105 | 0.955–1.286 |  |
| Subgraph by attribute | 0.000182573 | 0.000200995 | 0.996 | 0.806–1.283 |  |
| Expand (depth 2) | 0.002479413 | 0.002246603 | 1.145 | 0.893–1.479 |  |
| Remove nodes | 0.000469508 | 0.000476244 | 0.741 | 0.405–1.122 |  |
| Count edges | 0.000103488 | 0.000106588 | 0.975 | 0.833–1.181 |  |
| Random walks | 0.007126370 | 0.007477454 | 0.988 | 0.860–1.159 |  |
| Serialize to JSON string | 0.005996156 | 0.006119631 | 0.962 | 0.885–1.053 |  |
| Load from JSON string | 0.009995008 | 0.011028460 | 0.934 | 0.865–1.014 |  |
| Batch paths / individual calls | 0.045958164 | 0.047485936 | 0.979 | 0.773–1.223 |  |

Exclusions: none.

### networkx:20000:100000

| Workload | Before | After | Paired ratio | 95% interval | Screen flag |
|---|---:|---:|---:|---:|---|
| Build graph | 0.223662899 | 0.215654232 | 1.035 | 0.972–1.097 |  |
| BFS (full) | 0.089348552 | 0.091847613 | 0.947 | 0.844–1.047 |  |
| BFS (depth 3) | 0.000685469 | 0.000752656 | 0.801 | 0.695–0.908 | repeat required |
| BFS (edge filter) | 0.000105183 | 0.000103799 | 0.965 | 0.816–1.137 |  |
| DFS traversal | 0.099988411 | 0.097238379 | 1.050 | 0.963–1.158 |  |
| BFS search | 0.000178666 | 0.000160881 | 1.167 | 0.986–1.392 |  |
| Shortest path (unweighted) | 0.000257809 | 0.000242531 | 1.074 | 0.958–1.227 |  |
| Shortest path (Dijkstra) | 0.020557131 | 0.017041409 | 1.188 | 1.076–1.314 |  |
| Batch shortest paths | 0.054939466 | 0.057576551 | 0.975 | 0.937–1.029 |  |
| Distances from sources | 0.103900317 | 0.104580320 | 0.948 | 0.852–1.038 |  |
| Project graph | 0.013082969 | 0.012337248 | 1.024 | 0.927–1.140 |  |
| Weakly connected components | 0.007531408 | 0.006548372 | 1.071 | 0.911–1.231 |  |
| Strongly connected components | 0.007203482 | 0.007745653 | 0.939 | 0.797–1.070 |  |
| PageRank | 0.022361022 | 0.018706507 | 1.253 | 1.026–1.585 |  |
| Triangles | 0.015081332 | 0.015969710 | 0.944 | 0.851–1.054 |  |
| Core number | 0.013889753 | 0.013315123 | 1.044 | 0.813–1.291 |  |
| Label propagation | 0.021740409 | 0.019433309 | 0.994 | 0.790–1.166 |  |
| BFS levels | 0.010926181 | 0.012835143 | 0.880 | 0.643–1.181 |  |
| Betweenness (sampled) | 0.090386129 | 0.072352603 | 1.143 | 1.004–1.325 |  |
| Similarity | 0.021011597 | 0.020830660 | 1.197 | 0.955–1.626 |  |
| Most similar nodes | 0.078433593 | 0.080741576 | 1.033 | 0.877–1.176 |  |
| Leiden vs Louvain | 0.179820926 | 0.173155169 | 0.983 | 0.902–1.051 |  |
| Minimum spanning tree | 0.028064339 | 0.028426579 | 1.030 | 0.936–1.138 |  |
| k shortest paths | 0.353867327 | 0.358269277 | 1.029 | 0.930–1.183 |  |
| FastRP embeddings | 0.110218655 | 0.109336340 | 1.011 | 0.924–1.108 |  |
| node2vec walks | 0.048320327 | 0.045591035 | 1.196 | 0.942–1.562 |  |
| Grid: Dijkstra | 0.021695776 | 0.020582503 | 1.030 | 0.942–1.117 |  |
| Grid: A* (coordinates) | 0.002866728 | 0.002585064 | 1.120 | 0.980–1.296 |  |
| Grid: A* (precomputed) | 0.002230049 | 0.002624733 | 0.996 | 0.850–1.169 |  |
| Subgraph by ids | 0.008401887 | 0.008553138 | 1.124 | 0.887–1.539 |  |
| Subgraph by attribute | 0.003854489 | 0.003625087 | 1.013 | 0.860–1.200 |  |
| Expand (depth 2) | 0.008090236 | 0.008704547 | 0.871 | 0.728–1.012 |  |
| Remove nodes | 0.012933896 | 0.012923909 | 0.932 | 0.751–1.081 |  |
| Count edges | 0.001411904 | 0.001231378 | 1.161 | 1.035–1.304 |  |
| Random walks | 0.026348521 | 0.028656287 | 0.982 | 0.908–1.091 |  |
| Serialize to JSON string | 0.261671614 | 0.221271508 | 1.148 | 0.993–1.332 |  |
| Load from JSON string | 0.299666782 | 0.309296438 | 0.993 | 0.895–1.123 |  |
| Batch paths / individual calls | 2.941627963 | 2.599286485 | 1.188 | 1.073–1.357 |  |

Exclusions: none.

### libraries:facebook

| Workload | Before | After | Paired ratio | 95% interval | Screen flag |
|---|---:|---:|---:|---:|---|
| Build graph | 0.185327935 | 0.188624413 | 0.937 | 0.817–1.046 |  |
| Weakly connected components / fresh | 0.020992635 | 0.023957597 | 0.957 | 0.883–1.039 |  |
| Weakly connected components / reused | 0.006438214 | 0.006657224 | 1.054 | 0.936–1.217 |  |
| BFS levels from one node / fresh | 0.020064996 | 0.021400179 | 0.887 | 0.732–1.034 |  |
| BFS levels from one node / reused | 0.010138425 | 0.011409271 | 0.927 | 0.853–1.033 |  |
| Dijkstra from one node (weighted) / fresh | 0.039196323 | 0.040449188 | 0.934 | 0.811–1.062 |  |
| Dijkstra from one node (weighted) / reused | 0.016900900 | 0.020347608 | 0.857 | 0.708–1.006 |  |
| PageRank / fresh | 0.045115370 | 0.035724828 | 1.192 | 0.974–1.521 |  |
| PageRank / reused | 0.039528670 | 0.036149726 | 0.995 | 0.797–1.251 |  |
| Core number / fresh | 0.022819540 | 0.023293804 | 1.020 | 0.874–1.150 |  |
| Core number / reused | 0.014611061 | 0.014574433 | 1.053 | 0.944–1.159 |  |
| Local clustering / fresh | 0.027525265 | 0.027799621 | 1.009 | 0.903–1.114 |  |
| Local clustering / reused | 0.019710576 | 0.019911733 | 1.052 | 0.954–1.177 |  |
| Betweenness (exact) / fresh | 39.061692862 | 38.914553864 | 0.970 | 0.896–1.046 |  |
| Betweenness (exact) / reused | 37.639594684 | 37.561913425 | 0.999 | 0.982–1.023 |  |
| Communities (Leiden / Louvain) / fresh | 0.129768478 | 0.121737013 | 1.069 | 0.996–1.150 |  |
| Communities (Leiden / Louvain) / reused | 0.105590677 | 0.115186876 | 0.993 | 0.895–1.128 |  |
| Minimum spanning forest (weight) / fresh | 0.038461615 | 0.042309687 | 0.878 | 0.760–1.005 |  |
| Minimum spanning forest (weight) / reused | 0.018291601 | 0.018864090 | 0.889 | 0.765–0.999 |  |

Exclusions: Strongly connected components: directed-only operation on undirected dataset.

### libraries:github

| Workload | Before | After | Paired ratio | 95% interval | Screen flag |
|---|---:|---:|---:|---:|---|
| Build graph | 0.345017562 | 0.369203944 | 0.797 | 0.566–1.004 |  |
| Weakly connected components / fresh | 0.038068622 | 0.038406153 | 0.943 | 0.828–1.036 |  |
| Weakly connected components / reused | 0.012645445 | 0.012841910 | 1.075 | 0.839–1.411 |  |
| BFS levels from one node / fresh | 0.040777449 | 0.043816192 | 0.930 | 0.771–1.114 |  |
| BFS levels from one node / reused | 0.015943787 | 0.016587806 | 0.941 | 0.832–1.079 |  |
| Dijkstra from one node (weighted) / fresh | 0.086075061 | 0.084255706 | 1.074 | 0.956–1.227 |  |
| Dijkstra from one node (weighted) / reused | 0.033812698 | 0.036290761 | 0.939 | 0.912–0.967 |  |
| PageRank / fresh | 0.067885267 | 0.073926505 | 1.009 | 0.932–1.091 |  |
| PageRank / reused | 0.046963647 | 0.057018998 | 0.922 | 0.684–1.235 |  |
| Core number / fresh | 0.041459763 | 0.041110176 | 1.072 | 0.923–1.328 |  |
| Core number / reused | 0.027853796 | 0.034064296 | 0.827 | 0.669–1.002 |  |
| Local clustering / fresh | 0.049843379 | 0.057147851 | 0.914 | 0.829–0.990 |  |
| Local clustering / reused | 0.034497652 | 0.040821045 | 0.878 | 0.731–1.029 |  |
| Communities (Leiden / Louvain) / fresh | 0.259564094 | 0.259354125 | 0.962 | 0.885–1.037 |  |
| Communities (Leiden / Louvain) / reused | 0.246478878 | 0.231518773 | 0.885 | 0.625–1.111 |  |
| Minimum spanning forest (weight) / fresh | 0.063091777 | 0.070555972 | 0.989 | 0.894–1.130 |  |
| Minimum spanning forest (weight) / reused | 0.032850246 | 0.032198199 | 1.014 | 0.980–1.049 |  |

Exclusions: Strongly connected components: directed-only operation on undirected dataset; Betweenness (exact): existing nodes*edges > 5e9 budget.

### libraries:kron

| Workload | Before | After | Paired ratio | 95% interval | Screen flag |
|---|---:|---:|---:|---:|---|
| Build graph | 1.362723053 | 1.419628388 | 0.957 | 0.800–1.134 |  |
| Weakly connected components / fresh | 0.127173413 | 0.105561705 | 1.177 | 0.954–1.484 |  |
| Weakly connected components / reused | 0.040372443 | 0.041866181 | 0.895 | 0.712–1.048 |  |
| Strongly connected components / fresh | 0.097824725 | 0.100790536 | 1.086 | 0.786–1.565 |  |
| Strongly connected components / reused | 0.063225896 | 0.059178531 | 1.252 | 0.993–1.765 |  |
| BFS levels from one node / fresh | 0.079077016 | 0.092166354 | 0.857 | 0.668–1.088 |  |
| BFS levels from one node / reused | 0.018329210 | 0.019895015 | 1.109 | 0.907–1.389 |  |
| Dijkstra from one node (weighted) / fresh | 0.136848032 | 0.135771120 | 1.029 | 0.836–1.247 |  |
| Dijkstra from one node (weighted) / reused | 0.043369818 | 0.040949492 | 1.129 | 0.934–1.368 |  |
| PageRank / fresh | 0.092608744 | 0.091697483 | 0.963 | 0.840–1.067 |  |
| PageRank / reused | 0.049417586 | 0.051676528 | 0.958 | 0.814–1.119 |  |
| Core number / fresh | 0.110792267 | 0.102253964 | 1.126 | 1.000–1.294 |  |
| Core number / reused | 0.070572213 | 0.059848457 | 1.081 | 0.967–1.264 |  |
| Local clustering / fresh | 0.214682172 | 0.205859683 | 1.165 | 0.997–1.509 |  |
| Local clustering / reused | 0.195376887 | 0.171499540 | 1.313 | 1.027–1.771 |  |
| Communities (Leiden / Louvain) / fresh | 0.851787523 | 0.889480617 | 0.973 | 0.860–1.098 |  |
| Communities (Leiden / Louvain) / reused | 0.740011314 | 0.813336069 | 0.991 | 0.893–1.128 |  |
| Minimum spanning forest (weight) / fresh | 0.244474654 | 0.256779628 | 0.899 | 0.786–1.006 |  |
| Minimum spanning forest (weight) / reused | 0.133305765 | 0.133173651 | 1.030 | 0.953–1.139 |  |

Exclusions: Betweenness (exact): existing nodes*edges > 5e9 budget.

### memory:10000:50000

| Workload | Before | After | Paired ratio | 95% interval | Screen flag |
|---|---:|---:|---:|---:|---|
| JSON file | 9,127,165 | 9,127,165 | 1.000 | 1.000–1.000 |  |
| Binary file | 3,965,548 | 3,965,548 | 1.000 | 1.000–1.000 |  |
| graph | 23,855,104 | 23,855,104 | 1.000 | 1.000–1.000 |  |
| graph_no_attrs | 8,400,896 | 8,400,896 | 1.001 | 1.000–1.004 |  |
| load_json | 70,397,952 | 70,205,440 | 1.003 | 1.001–1.004 |  |
| save_json | 12,697,600 | 12,673,024 | 0.997 | 0.987–1.005 |  |

Exclusions: none.

### memory:100000:500000

| Workload | Before | After | Paired ratio | 95% interval | Screen flag |
|---|---:|---:|---:|---:|---|
| JSON file | 95,167,868 | 95,167,868 | 1.000 | 1.000–1.000 |  |
| Binary file | 43,525,674 | 43,525,674 | 1.000 | 1.000–1.000 |  |
| graph | 239,894,528 | 239,894,528 | 1.000 | 1.000–1.000 |  |
| graph_no_attrs | 85,331,968 | 85,331,968 | 1.000 | 1.000–1.000 |  |
| load_json | 696,918,016 | 696,948,736 | 1.000 | 1.000–1.000 |  |
| save_json | 129,515,520 | 129,497,088 | 1.000 | 1.000–1.001 |  |

Exclusions: none.

## Independent follow-up

Every broad-screen interval wholly below 1 was repeated using identical
frozen artifacts, datasets, operations and thread counts. Fresh/reused
projection modes remain the same. The targeted repeat uses one warmup and
seven measured samples in each of 20 alternating process pairs. It is a
separate experiment, not extra samples appended to a selected screen.

| Workload | Before (s) | After (s) | Paired ratio | 95% interval |
|---|---:|---:|---:|---:|
| confirm-networkx:small: Most similar nodes | 0.004158148 | 0.004545146 | 0.889 | 0.809–0.971 |
| confirm-networkx:large: BFS (depth 3) | 0.000939149 | 0.000985964 | 0.964 | 0.909–1.025 |
| confirm-libraries:facebook: Minimum spanning forest (weight) / reused | 0.020276113 | 0.019337078 | 0.999 | 0.927–1.079 |
| confirm-libraries:github: Dijkstra from one node (weighted) / reused | 0.033871974 | 0.034526201 | 0.987 | 0.939–1.041 |
| confirm-libraries:github: Local clustering / fresh | 0.049075232 | 0.047666296 | 1.023 | 0.986–1.061 |
| confirm-networkx:small: Most similar nodes (60 pairs, 25 samples/process) | 0.004107428 | 0.004076922 | 0.999 | 0.966–1.032 |
| confirm-networkx:small: Most similar nodes (60 pairs, original 7 samples/process) | 0.004386399 | 0.004430510 | 0.936 | 0.878–0.999 |

The large BFS screen suggested a 20% loss, but its independent repeat was
0.964 (0.909–1.025), which is inconclusive. Similarity was slower in the screen
and first repeat. The 60-pair, 25-sample repeat measured 0.999
(0.966–1.032), but returning to seven samples/process measured 0.936
(0.878–0.999). Changing inner sample count changes how much warmed execution
contributes to the process median; the longer repeat cannot dismiss the
short-run difference as noise. Its cause is unresolved. The final short-run
point estimate suggests approximately 6.9% more time, with an interval spanning
roughly 0.1% to 13.9% more time. These nominal intervals do not prove a >5%
regression, and screening/extra follow-ups limit inference. The PR stays draft
for further investigation rather than declaring every workload unaffected.
The other repeated algorithm intervals include 1.

The deletion-control follow-up and its limits are in [node_removal.md](node_removal.md).

## Reproduction

Use [the benchmark workflow](../benchmarks/README.md) to build both committed
revisions with one lockfile and run the complete inventory on one machine.
The measured builds used the same archive/build steps as the portable helper;
the candidate archive captured the working-tree core source identified above.
Run the broad comparison with `--pairs 6 --repeats 3`, focused comparison with
`--pairs 20 --samples 3`, and targeted confirmation with `--pairs 20 --repeats 7`.
The larger similarity repeats add `--cases confirm-networkx:small --pairs 60`
with `--repeats 25` and, independently, `--repeats 7`.
Never overlap benchmark jobs, builds or tests.

Raw data: [full screen](revision_benchmarks_samples.json),
[20-pair confirmation](revision_confirmation_samples.json),
[60-pair warmed similarity confirmation](similarity_confirmation_samples.json),
[60-pair original-protocol similarity confirmation](similarity_short_run_samples.json),
[focused deletion](node_removal_samples.json) and
[larger-batch deletion controls](node_removal_controls_samples.json).
