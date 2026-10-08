# NodeView.type lookup optimization

`NodeView.type` copied every node attribute through `node.attr.get("type")`.
Use the existing `node.attr_get("type")` method to fetch one value directly.
This changes lookup work from proportional to the number of attributes to a
single dictionary lookup, while preserving missing values and explicit `None`.

## Focused measurements

Two independent runs of 12 baseline/candidate process pairs on the same machine.
Order alternates per pair; workers execute serially on one pinned CPU with
`PYTHONHASHSEED=0` and `RAYON_NUM_THREADS=1`. Baseline and candidate Python
packages were frozen before timing and use the identical prebuilt extension.
Each process reports three inner repetitions; confidence intervals resample
paired process medians, not individual inner timings. Raw samples, interpreter,
affinity, wrapper hashes, and extension hashes are in `samples.json`.

Filters operate on 1,000 nodes. The selective filter retains 10 nodes (1%).
Attribute counts below exclude the `type` attribute. Times and intervals in the
table are from the independent repeat; speedups are geometric means of paired
ratios, which can differ from ratios of the reported medians.

| Operation | Unrelated attributes | Before median | After median | Paired speedup (95% bootstrap interval) |
|---|---:|---:|---:|---:|
| Type lookup | 0 | 154 ns | 100 ns | 1.50x (1.38–1.60) |
| Type lookup | 8 | 171 ns | 97 ns | 1.61x (1.40–1.77) |
| Type lookup | 64 | 318 ns | 100 ns | 2.96x (2.59–3.23) |
| Type lookup | 512 | 2,222 ns | 96 ns | 21.56x (18.85–23.90) |
| Filter, no matches | 8 | 449 us | 369 us | 1.16x (1.06–1.24) |
| Filter, 1% matches | 8 | 457 us | 377 us | 1.11x (0.95–1.22), inconclusive |
| Filter, no matches | 512 | 2,777 us | 375 us | 7.04x (6.25–7.59) |
| Filter, 1% matches | 512 | 2,765 us | 387 us | 6.94x (6.33–7.38) |
| Unchanged attribute-filter control | 8 | 460 us | 459 us | 0.91x (0.75–1.02), inconclusive |
| Unchanged attribute-filter control | 512 | 2,782 us | 2,819 us | 0.99x (0.97–1.01), inconclusive |

The initial run measured a small control slowdown at 512 attributes (0.97x,
interval 0.95–0.99). The independent repeat did not confirm that slowdown.
These results describe the focused workloads, not overall library performance.

## Correctness and limits

Before and after: all 23 filter tests pass, including new checks for missing,
null, string, and integer type values and live attribute updates. After: all
41 filter, edge-filter, and handle tests pass. `git diff --check` passes.

The wider 48-case selection produces the same result for baseline and candidate:
46 passed, one skipped, one failure. `test_label_and_edge_type_counts` fails
because the available prebuilt extension lacks `Vertex.labels()`, which newer
Python tests expect. No Rust sources or public method signatures changed.

The full workload inventory in `benchmarks/README.md`, fresh Rust builds, fmt,
and Clippy have not been run: this environment has no Rust toolchain and the
available extension predates the current main branch. Keep this PR as a draft
pending validation with a current build and the repository's broad suite.

To reproduce the focused comparison, freeze the two source trees with their
compiled extensions and run:

```bash
python benchmarks/compare_nodeview_type.py BASELINE CANDIDATE --pairs 12 --output results.json
```
