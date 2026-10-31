# Tiny graph decode experiment

Final runtime change: use the core graph’s seeded `StrMap` (foldhash) for the per-load Python short-string cache instead of std HashMap/SipHash. This retains string sharing, cache lifetime, format validation and all public APIs. Core loader code is unchanged. No new dependency or global Python-object cache is introduced.

## Setup

CPython 3.13.5, conventional GIL build, optimized Rust release artifacts, same dependency versions. Six independent process pairs alternate artifact order. Each case retains five inner repetition samples; confidence intervals resample process pairs, not inner repetitions. Baseline source: `2323515` (PR #138). Raw final data and extension hashes are in `measurements.json`.

The worker covers 0–10k nodes, two edges/node, scalar, many-unique-attribute, nested, embedding, indexed and Unicode profiles. Timings include graph disposal, as part of a short-lived handler lifecycle. Tests verify loaded values, indexes, shared short strings and adjacency. These are warm-cache workload measurements, not HTTP p95 or universal speedups. CIs are exploratory, without multiple-comparison adjustment.

## Binary decoding

| Nodes | Payload | Before µs | After µs | Paired speedup (95% bootstrap CI) |
| ---: | --- | ---: | ---: | --- |
| 0 | scalar | 1.14 | 1.22 | 0.963× (0.899–1.040) |
| 1 | scalar | 3.61 | 3.75 | 0.979× (0.921–1.042) |
| 10 | scalar | 21.68 | 20.73 | 1.053× (1.009–1.099) |
| 100 | scalar | 203.23 | 185.87 | 1.088× (1.069–1.112) |
| 1000 | scalar | 2044.04 | 1876.31 | 1.065× (0.990–1.120) |
| 10000 | scalar | 26238.56 | 25092.50 | 1.028× (0.960–1.105) |
| 10 | unique | 106.69 | 82.92 | 1.308× (1.258–1.383) |
| 10 | nested | 41.52 | 38.41 | 1.104× (1.051–1.174) |
| 10 | embedding | 187.12 | 187.35 | 0.999× (0.955–1.044) |
| 10 | indexed | 25.26 | 24.09 | 0.979× (0.824–1.088) |
| 10 | unicode | 23.18 | 21.88 | 0.978× (0.787–1.114) |

The main gain is hashing attribute names and repeated short strings. The unique profile adds 32 distinct attribute keys/values per node. Ordinary ten-node graphs have a small gain; 100-node and attribute-heavy cases have clearer gains. Empty, one-node, numeric-embedding and some larger cases remain inconclusive.

## Independent controls

The initial screen showed shifts in encoding controls. This independent eight-pair run measures encoding before any load operations to reduce prior-load allocator/cache-state effects. Numeric-heavy JSON loading is another control. Final intervals include 1 for all four focused controls; no clear regression was detected.

| Control | Before µs | After µs | Paired speedup (95% CI) |
| --- | ---: | ---: | --- |
| encode/1/scalar | 1.22 | 1.20 | 1.012× (0.977–1.044) |
| encode/1000/scalar | 534.11 | 522.47 | 1.016× (0.988–1.047) |
| encode/10/unicode | 6.43 | 6.27 | 1.020× (0.986–1.054) |
| decode_json/10/embedding | 580.17 | 581.97 | 1.003× (0.968–1.038) |

## Persisted read/restore

Three additional process pairs use the SQLite WAL/FULL integration harness on `/workspace` overlay storage. Raw binary snapshots, one-score changes shown below; all-score update results and write controls are retained in the raw data. Read timers include SELECT + graph reconstruction, with disposal outside the timer. Medians across three process medians are illustrative; this is not an Ironweaver DB, cold-storage, concurrent-client or HTTP benchmark. Writes still include serialization and synchronous commits, but the final change targets only loading.

| Nodes/graph | Graphs/read | Before µs | After µs |
| ---: | ---: | ---: | ---: |
| 1 | 1 | 23.87 | 24.12 |
| 1 | 100 | 764.67 | 775.27 |
| 10 | 1 | 43.29 | 44.25 |
| 10 | 100 | 2770.90 | 2558.27 |
| 100 | 1 | 216.64 | 190.49 |
| 100 | 100 | 24219.10 | 22659.17 |

The ordinary single ten-node persisted read is inconclusive. The 100-node read shows a clearer improvement, and batch observations are in the raw data. Durable write/sync latency is not expected to improve from a loader cache change.

## Profiling guidance

Temporary native timers on ordinary unindexed graphs measured approximately 8 µs parsing and 11 µs building a ten-node graph; node/edge Python attributes accounted for about 5 µs of the build time. GC setup was under 1 µs. `profiling.json` contains the phase medians and units. Timers introduce overhead, build contains the attribute timers, and the figures are not additive percentages. Instrumentation was removed from the final runtime.

A broader experiment also replaced the core edge-ID lookup map, but that part was reverted after a numeric-heavy JSON control slowed down. Only the Python cache change is kept. Discovery data is archived in the session scratch workspace; all final comparison samples are retained here.

## Validation and reproduction

All 1,230 Python tests pass; Rust tests, Clippy and formatting checks pass. New regression tests cover string sharing across nodes/edges/metadata, Unicode, indexes and edge order for binary and JSON loads. No timing assertions are added to CI.

Build/install baseline and candidate with the same optimized configuration into separate frozen directories, then alternate fresh processes with each directory on PYTHONPATH:

```bash
python benchmarks/compare_binary_decode.py
python benchmarks/compare_decode_controls.py
python benchmarks/persist_binary.py --directory /path/on/target/filesystem --codecs raw --output persistence.json
```

Keep all raw samples, run no builds/tests concurrently with measurements, and compare independent process pairs.
