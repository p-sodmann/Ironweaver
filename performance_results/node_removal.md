# Node deletion: parallel adjacency cleanup

Deleting a node with many parallel edges repeatedly scanned the same neighbor
list: 1,000 / 4,000 / 16,000 outgoing edges took approximately 0.26 / 3.92 /
61.72 ms through the Python API in the initial seven-run investigation.
The roughly 16-fold cost increase for four times as many edges identified
quadratic work in `Graph::try_remove_node`.

The change removes all parallel edges during the first scan of the opposite
adjacency list. Later visits find those edge slots empty. Single-entry lists
are cleared directly, and deleting a leaf retains the original single-edge
scan. There are no new allocations or public API/binding changes. Surviving
adjacency order is preserved. Work becomes linear in the removed node's
incident edges plus the adjacency entries of its distinct neighbors; those
neighbor lists can contain unrelated surviving edges.

## Experiments

1. Collect, sort and deduplicate neighbor handles, then clean their lists once.
   Pilot parallel-edge gains were large, but ordinary-star latency rose
   about 46% and shuffled-star latency about 59%. Rejected.
2. Remove parallel edges inside the first adjacency scan. This eliminated the
   quadratic work without an allocation, but still added overhead to stars.
3. Add direct clearing for single-entry lists and retain the original scan for
   leaves. This is the selected implementation. Final measurements below use
   independent runs after selecting it, not the exploratory pilot samples.

## Measurement protocol

- Baseline: `7e7b7fa40e3224b512d59d046f28b5bc0ffe7144`.
- Rust 1.99.0, standard Cargo release profile, identical dependency lockfile and
  benchmark source for both binaries. No target-specific compiler flags.
  Cargo.lock SHA-256: `8a88c8fe93618bb1741941a80608c7a75c627ed2fe60d2b70b25d7cd6272623a`.
- Linux x86-64 cloud VM, Intel Xeon Platinum 8370C. Both binaries pinned to the
  same available logical CPU. No concurrent builds or tests.
- 20 independent process pairs per case, alternating baseline/candidate and
  candidate/baseline order. Each process excludes three warmup samples and
  records three measured samples, each averaging four deletions (1,000 for
  tiny graphs).
- The timer covers `Graph::remove_node` only. Fixture construction, cloning,
  assertions, and graph/result destruction are outside the timer. Fixtures
  use unit payloads. These are core deletion measurements, not whole-application
  throughput or allocation-inclusive graph lifecycle timings.
- Before/after values are medians of per-process medians. Speedup is the
  geometric mean of paired ratios. A seeded 10,000-resample paired bootstrap
  gives a 95% interval, resampling process pairs rather than correlated inner
  samples. Intervals containing 1 do not establish a speedup; effects below
  5% are treated as practically inconclusive even if statistically detectable.
- The raw JSON retains all measured samples, run order and binary SHA-256s.
  Intervals describe this machine/run, not guaranteed speedups on all hardware.

## Results

| Case | Size | Before (µs) | After (µs) | Paired speedup | 95% interval | Faster pairs |
|---|---:|---:|---:|---:|---:|---:|
| parallel-out | 1,000 | 251.770 | 20.360 | 12.673× | 12.340–13.055× | 20/20 |
| parallel-out | 4,000 | 3,789.853 | 82.377 | 45.003× | 43.127–46.381× | 20/20 |
| parallel-out | 16,000 | 58,902.253 | 343.689 | 170.130× | 162.775–176.811× | 20/20 |
| parallel-in | 16,000 | 59,512.938 | 341.180 | 175.506× | 171.416–180.116× | 20/20 |
| bidirectional | 16,000 | 29,876.997 | 357.968 | 84.041× | 82.204–85.843× | 20/20 |
| mixed | 16,000 | 10,319.071 | 434.075 | 23.803× | 22.890–24.633× | 20/20 |
| star | 16,000 | 244.371 | 208.018 | 1.191× | 1.152–1.233× | 20/20 |
| shuffled-star | 16,000 | 399.426 | 275.262 | 1.439× | 1.376–1.502× | 20/20 |
| star | 4 | 0.139 | 0.133 | 1.058× | 1.024–1.097× | 18/20 |
| self-loops | 16,000 | 172.241 | 167.950 | 1.012× | 0.988–1.036× | 14/20 |
| isolated | 0 | 0.072 | 0.071 | 1.057× | 1.001–1.156× | 12/20 |
| leaf-out | 16,000 | 8.104 | 8.100 | 1.008× | 0.997–1.020× | 8/20 |
| leaf-in | 16,000 | 8.109 | 8.160 | 0.975× | 0.929–1.005× | 9/20 |

Parallel-edge cases improve decisively beyond run-to-run variation; all 20
pairs favor the candidate. At 16,000 outgoing edges the paired speedup is
170× (95% interval 163–177×). The 16,000-edge star controls also improve.
Leaf, self-loop and isolated-node effects are too small or uncertain to
claim practical improvements. The four-edge star changes by only a few
nanoseconds; that result is not an application-level speed claim.

[Raw measurements](node_removal_samples.json).

`mixed` interleaves 16,000 removed edges with 16,000 surviving edges across
16 neighbors. `star` and `shuffled-star` have one edge per neighbor, the latter
in seeded shuffled insertion order. `leaf-out`/`leaf-in` remove one edge from
an adjacency list containing another 16,000 surviving edges. Self-loops and
isolated nodes are controls.

## Reproduction

From the candidate checkout, with the Rust toolchain active:

```bash
set -eu
repo=$(pwd)
base=$(mktemp -d)
git archive 7e7b7fa40e3224b512d59d046f28b5bc0ffe7144 | tar -x -C "$base"
mkdir -p "$base/crates/ironweaver-core/examples"
cp crates/ironweaver-core/examples/bench_remove_node.rs "$base/crates/ironweaver-core/examples/"
# Cargo.lock is ignored by the repository; use the same resolved dependencies.
# Generate it first with cargo generate-lockfile if absent.
cp Cargo.lock "$base/Cargo.lock"
cargo build --locked --release --manifest-path "$base/Cargo.toml" \
  -p ironweaver-core --example bench_remove_node
cp "$base/target/release/examples/bench_remove_node" /tmp/remove-node-baseline
cd "$repo"
cargo build --locked --release -p ironweaver-core --example bench_remove_node
cp target/release/examples/bench_remove_node /tmp/remove-node-candidate
python benchmarks/compare_remove_node.py \
  /tmp/remove-node-baseline /tmp/remove-node-candidate \
  --pairs 20 --samples 3 --output /tmp/node-removal-results.json
```

Run comparisons serially with no compilation/test load. Increase repetitions
or rerun independently before treating a small difference as an improvement.
Do not turn timing ratios into CI assertions on shared runners.

## Correctness validation

- `cargo test -p ironweaver-core`: 153 passed, including three new regression
  tests and three doctests.
- Rebuilt the editable extension, then `python -m pytest -q`: 830 passed.
- `cargo fmt --all --check` and
  `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- New tests cover mixed parallel edges and self-loops, both edge directions,
  dense and sparse edge IDs, stable surviving adjacency order, repeated node
  and edge slot reuse against a seeded edge-list oracle, and exactly-once
  payload destruction. Existing invariant-error and memory-accounting tests
  also pass.
- `src/` and `python/` (bindings and Python API) are unchanged.
