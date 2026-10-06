# Node deletion: measured adjacency batching

Repeated single-edge adjacency scans made parallel-edge node deletion quadratic.
The original investigation showed roughly 16-fold growth when quadrupling the
number of parallel edges. The final candidate defers only adjacency compaction;
it preserves the original edge removal, payload destruction and free-slot
reuse order. The bindings and Python sources are unchanged.

## Selected implementation and tradeoffs

Single-entry opposite lists are cleared directly. Other small deletions keep
individual handle-comparison scans. Deferred grouping is considered only when
the removed node has at least 32 edges in that direction and the opposite list
has at least 256 entries. Deferred `(neighbor, edge)` pairs are sorted by neighbor.
Groups of at least 16 removed edges clean the list once using live-handle checks;
smaller groups retain individual comparisons. Surviving adjacency order stays
stable, and self-loops are removed once.

This removes repeated scans for large parallel groups. Grouping adds a sort
and temporary O(deferred incident edges) scratch storage, reused between the
two directions; it does not add persistent graph storage. A 16,000-edge deferred
list uses approximately 256 KiB of scratch capacity on this target. General
sorting costs O(d log d), in addition to adjacency cleanup. These are explicit
costs, not an allocation-free or universally linear deletion claim.

## Alternatives and regression discovered

- Sorting/deduplicating every neighbor improved parallel edges but slowed
  ordinary/shuffled stars in the pilot. Rejected.
- Removing all parallel edges inside the first opposite-list scan avoided
  allocations, but an added control exposed an approximately 8× loss when
  deleting only two edges into large hubs. It also changed drop/slot-reuse order.
  This implementation was replaced.
- The selected hybrid preserves removal order, avoids grouping small deletions,
  and uses cheap comparisons when a neighbor has only a few removed edges.
  Final measurements use freshly matched artifacts, not the exploratory data.

## Measurement protocol

Baseline is `7e7b7fa40e3224b512d59d046f28b5bc0ffe7144`. Both release binaries
use Rust 1.99.0, one copied lockfile, identical benchmark source and flags on
the same Xeon Platinum 8370C VM. They are pinned to the same logical CPU.
There are 20 alternating process pairs per fixture, three warmups and three
samples per process, averaging four deletions per sample (1,000 for tiny cases).
Construction, cloning, validation and destruction are outside the timer;
fixtures use unit payloads. Scratch allocation inside deletion is timed.
No builds, tests or other benchmarks ran concurrently.

Values are medians of process medians. Ratios are paired geometric means with
seeded 10,000-resample 95% bootstrap intervals. An interval containing 1 is
inconclusive; small statistical effects are not application-level gains.
Intervals characterize this VM/run, not every machine.

## Complete focused results

| Fixture | Size | Before (µs) | After (µs) | Paired speedup | 95% interval | Faster pairs |
|---|---:|---:|---:|---:|---:|---:|
| parallel-out | 1,000 | 279.486 | 12.130 | 23.617× | 20.659–27.025× | 20/20 |
| parallel-out | 4,000 | 4468.861 | 46.819 | 87.561× | 77.901–96.773× | 20/20 |
| parallel-out | 16,000 | 68282.927 | 217.963 | 323.809× | 293.959–358.081× | 20/20 |
| parallel-in | 16,000 | 66094.033 | 250.365 | 262.891× | 227.263–296.053× | 20/20 |
| bidirectional | 16,000 | 33490.976 | 255.087 | 132.932× | 120.977–145.485× | 20/20 |
| mixed | 16,000 | 12492.826 | 412.499 | 31.821× | 28.460–36.008× | 20/20 |
| star | 16,000 | 289.574 | 228.326 | 1.284× | 1.196–1.400× | 19/20 |
| shuffled-star | 16,000 | 463.625 | 337.609 | 1.357× | 1.257–1.466× | 18/20 |
| star | 4 | 0.139 | 0.134 | 1.052× | 0.993–1.110× | 17/20 |
| self-loops | 16,000 | 189.117 | 204.809 | 0.986× | 0.820–1.157× | 10/20 |
| isolated | 0 | 0.074 | 0.074 | 0.871× | 0.764–0.967× | 5/20 |
| leaf-out | 16,000 | 12.559 | 11.133 | 1.067× | 0.876–1.313× | 10/20 |
| leaf-in | 16,000 | 10.680 | 9.908 | 1.055× | 0.949–1.190× | 11/20 |
| two-hubs-out | 16,000 | 19.968 | 18.730 | 0.984× | 0.904–1.064× | 13/20 |
| two-hubs-in | 16,000 | 19.519 | 21.817 | 0.926× | 0.822–1.036× | 11/20 |
| fan-survivors | 1,024 | 75.231 | 73.543 | 0.923× | 0.764–1.088× | 8/20 |
| dense | 512 | 328.719 | 327.875 | 0.921× | 0.774–1.091× | 9/20 |

Parallel-edge improvements are far beyond run noise: at 16,000 outgoing edges,
68.3 ms becomes 218 µs, with a paired gain of 324× (294–358×). Large ordinary
and shuffled stars also improve. No gain is claimed for the remaining controls.

Size is incident edge count for parallel/star/loop fixtures, surviving-edge
count for leaves, survivors per hub for two-hub fixtures, and neighbor count
for fan/dense fixtures. `mixed` interleaves 16,000 removed and 16,000 surviving
edges across 16 neighbors. Two-hub fixtures remove only two edges while 32,000
edges survive. The fan has 64 unrelated incoming edges per neighbor; the dense
fixture has a fully connected surviving neighborhood, including self-loops.

## Independent larger-batch controls

Nine controls were repeated in 30 alternating process pairs, three measured
samples after three warmups, averaging 16 deletions per sample (10,000 for tiny
fixtures). This reduces timer noise, but several intervals still remain wide.

| Fixture | Size | Before (µs) | After (µs) | Paired ratio | 95% interval |
|---|---:|---:|---:|---:|---:|
| star | 4 | 0.156 | 0.153 | 1.041 | 0.989–1.098 |
| self-loops | 16,000 | 236.056 | 212.359 | 1.063 | 0.926–1.181 |
| isolated | 0 | 0.087 | 0.086 | 1.049 | 0.950–1.168 |
| leaf-out | 16,000 | 11.626 | 10.445 | 0.985 | 0.889–1.088 |
| leaf-in | 16,000 | 10.422 | 10.004 | 1.080 | 0.989–1.187 |
| two-hubs-out | 16,000 | 22.762 | 22.454 | 0.910 | 0.759–1.061 |
| two-hubs-in | 16,000 | 25.994 | 29.382 | 0.961 | 0.814–1.149 |
| fan-survivors | 1,024 | 114.314 | 132.922 | 0.898 | 0.769–1.046 |
| dense | 512 | 339.778 | 325.153 | 1.028 | 0.921–1.144 |

The tiny isolated-node slowdown in the first run did not persist in the repeat.
All control-repeat intervals include 1; this does not rule out moderate losses
within their wide intervals. In particular, the two-hub control no longer
reproduces the original 8× slowdown, but these measurements do not establish
strict equivalence or a small improvement for that case.

The [complete 143-metric broad screen and algorithm confirmations](revision_benchmarks.md)
provide the required same-machine check beyond deletion. Short-run similarity
results remain slower and inconsistent with a longer-sample repeat, so the PR
stays draft; this report does not claim a regression-free change. No historical
cross-machine timings were used.

## Reproduction and validation

Follow [benchmarks/README.md](../benchmarks/README.md) for matched builds and
both complete suites. The control repeat uses:

```bash
python benchmarks/compare_remove_node.py /tmp/revision-builds/baseline/bench_remove_node /tmp/revision-builds/candidate/bench_remove_node \
  --pairs 30 --samples 3 --batch 16 --small-batch 10000 \
  --cases star:4,self-loops:16000,isolated:0,leaf-out:16000,leaf-in:16000,two-hubs-out:16000,two-hubs-in:16000,fan-survivors:1024,dense:512 \
  --output /tmp/deletion-controls.json
```

Regression tests cover surviving adjacency order, mixed directions and loops,
dense/sparse edge IDs, stale handles, repeated slot reuse, exactly-once payload
destruction, and exact original payload destruction/free-slot reuse order.
Validation passed after rebuilding the editable extension: 154 Rust tests
(including three doctests), 833 Python tests, `cargo fmt --all --check`, and
`cargo clippy --workspace --all-targets -- -D warnings`. The pure core has no
PyO3 dependency, and `src/`/`python/` match the baseline.

[Raw focused samples](node_removal_samples.json),
[raw control repeat](node_removal_controls_samples.json).
