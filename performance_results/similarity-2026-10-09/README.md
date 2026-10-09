# Adaptive pairwise neighborhood similarity

`crates/ironweaver-core/src/algo/similarity.rs::common` now uses a galloping
intersection for neighbor lists whose lengths differ by at least 32 times.
Each search starts at the previous position, finds an exponential upper
bound, then binary-searches within that bound. Comparable degrees retain
the original linear merge. Pairs whose potential matches fit in a prefix
of at most four times the shorter list also retain the merge, which can stop
early in that case.

The search lives in a separate helper with inlining disabled, while the
merge wrapper is marked inline. This keeps the existing merge loop compact
in generated code; putting both loops in the wrapper regressed balanced
controls during validation.

This avoids scanning most of a hub's neighbors for a low-degree partner.
The change adds no allocations and sums matching weights in the same
ascending node order. Public APIs and score formulas are unchanged.

## Correctness

Before the production change, the existing core suite passed (160 tests
and 6 doctests), the two added regression tests passed, and a freshly built
Python extension passed all 876 Python tests. After the final change,
162 core tests, 6 doctests and all 876 Python tests passed. Formatting,
Clippy across the workspace/all targets with warnings denied, and the
no-PyO3 core dependency check passed.

The new tests compare all six similarity metrics against a brute-force
reference on hub/leaf pairs, including reversed queries, isolated nodes,
parallel edges, self-loops, and all three projection directions. A direct
intersection test checks bit-for-bit floating-point results in both input
orientations, including empty rows, no matches, early/late matches, and
sizes around the switching boundary.

## Measurement protocol

The benchmark calls the public Rust `similarity` function with 1,024 pairs
per call, alternating hub/partner orientations. Timings include validation,
the simple undirected view, scoring, and result allocation; constructing
the graph and projection is excluded. Each case is warmed up, then sampled
for at least 200 ms and at least three complete calls.

Six separate before/after process pairs used frozen release executables,
alternating execution order, CPU 0 affinity and one Rayon thread. Builds
and tests finished before sampling. Both executables use Rust 1.99.0,
Cargo's release profile and the same Cargo.lock, starting from commit
`9cec233619dbb34263f02c3275cec2beb207c732`. The host reports an AMD EPYC
9V74 CPU. Binary and lock hashes are in `raw/metadata.json`.

The graph has one hub and sixteen partners, with matched neighbors spread
through the hub's row, confined to its prefix/suffix, or disjoint from it.
Balanced rows at degrees 128 and 4,096 are controls. Every benchmark checks
result lengths and finite scores; common-neighbor scores also have exact
expected-value assertions.

Table runtimes are arithmetic means across processes. Changes and pointwise
95% confidence intervals use paired log runtime ratios and a Student-t
interval with five degrees of freedom. Negative changes mean faster.
These are synthetic, single-threaded Rust API workloads; they do not
establish the same speedup for a whole application or the Python API.

The executable benchmark is `crates/ironweaver-core/examples/similarity_bench.rs`:

```sh
RAYON_NUM_THREADS=1 cargo run -p ironweaver-core --release --example similarity_bench
```

Use identical benchmark sources on the before/after checkouts, save both
release executables, then run the paired sampler:

```sh
python performance_results/similarity-2026-10-09/run_paired.py BEFORE_BINARY AFTER_BINARY OUTPUT_DIRECTORY
```

`summary.csv` contains the unrounded results. `raw/` contains all twelve
process samples and their metadata. `validation-logs.zip` contains baseline
and final test/build logs and dependency information.

## Before/after results

Hub/leaf spread and disjoint workloads at degrees 4,096/8 and 32,768/8
improve by 88–94% in the paired estimates. The balanced and prefix controls
have confidence intervals overlapping zero; they establish no confirmed
regression or improvement. At degrees 4,096/128, Jaccard and Adamic–Adar
improve, while common-neighbor count is inconclusive. All samples are retained.

| Hub degree / other degree / position / metric | Before (ms) | After (ms) | Change [95% CI] |
|---|---:|---:|---:|
| 128/1/suffix/common_neighbors | 0.146 | 0.034 | -76.4% [-78.8, -73.6] |
| 128/1/suffix/jaccard | 0.143 | 0.035 | -75.6% [-77.0, -74.0] |
| 128/1/suffix/adamic_adar | 0.146 | 0.039 | -73.5% [-75.1, -71.9] |
| 4096/8/spread/common_neighbors | 3.818 | 0.423 | -88.9% [-89.7, -88.1] |
| 4096/8/spread/jaccard | 4.008 | 0.420 | -89.5% [-90.1, -88.9] |
| 4096/8/spread/adamic_adar | 3.759 | 0.435 | -88.4% [-89.4, -87.4] |
| 32768/8/spread/common_neighbors | 29.235 | 1.892 | -93.5% [-94.0, -93.0] |
| 32768/8/spread/jaccard | 32.131 | 1.892 | -94.1% [-94.6, -93.6] |
| 32768/8/spread/adamic_adar | 28.591 | 1.895 | -93.4% [-93.6, -93.2] |
| 4096/8/prefix/common_neighbors | 0.222 | 0.226 | +2.1% [-2.3, +6.8] |
| 4096/8/prefix/jaccard | 0.224 | 0.237 | +5.6% [-5.2, +17.5] |
| 4096/8/prefix/adamic_adar | 0.264 | 0.269 | +1.8% [-0.9, +4.5] |
| 4096/8/disjoint/common_neighbors | 3.558 | 0.424 | -88.1% [-88.4, -87.8] |
| 4096/8/disjoint/jaccard | 3.633 | 0.431 | -88.1% [-88.8, -87.4] |
| 4096/8/disjoint/adamic_adar | 3.668 | 0.428 | -88.3% [-89.1, -87.5] |
| 4096/128/spread/common_neighbors | 6.678 | 2.739 | -46.3% [-73.8, +9.9] |
| 4096/128/spread/jaccard | 3.899 | 2.699 | -30.7% [-34.6, -26.6] |
| 4096/128/spread/adamic_adar | 5.031 | 2.608 | -48.1% [-51.0, -45.0] |
| 128/128/spread/common_neighbors | 0.182 | 0.159 | -10.8% [-28.9, +12.0] |
| 128/128/spread/jaccard | 0.170 | 0.160 | -6.0% [-16.4, +5.7] |
| 128/128/spread/adamic_adar | 0.901 | 0.919 | +2.4% [-10.1, +16.5] |
| 4096/4096/spread/common_neighbors | 4.855 | 4.812 | -0.8% [-5.7, +4.3] |
| 4096/4096/spread/jaccard | 4.947 | 4.776 | -3.2% [-11.6, +6.0] |
| 4096/4096/spread/adamic_adar | 28.022 | 29.273 | +4.4% [-3.1, +12.5] |

## Independent balanced control check

The same final executables also ran six separate focused pairs with 500 ms
samples for degree 4,096/4,096. These results agree with the full-matrix
balanced controls: no confirmed change. The measurements remain separate
from the primary estimates; raw samples and metadata are in `repeat-balanced/`.

| Hub degree / other degree / position / metric | Before (ms) | After (ms) | Change [95% CI] |
|---|---:|---:|---:|
| 4096/4096/spread/common_neighbors | 4.748 | 4.678 | -1.5% [-7.5, +4.8] |
| 4096/4096/spread/jaccard | 4.994 | 4.738 | -5.2% [-11.3, +1.4] |
| 4096/4096/spread/adamic_adar | 27.986 | 28.404 | +1.6% [-3.6, +7.1] |

`prototype-measurements.zip` retains the intermediate measurements that
motivated the prefix fallback and helper split. These use earlier executables.
