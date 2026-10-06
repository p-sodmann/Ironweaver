# Adaptive traversal performance measurements — 2026-10-06

## Change and scope

Only core BFS and DFS visited membership changes. Small searches keep the
existing handle hash set. Once at least 128 nodes have been discovered and the
number of discovered nodes reaches ceil(arena slot bound / 64), membership
switches to a slot bitmap. The immutable single-source graph guarantees that
subsequent discovered handles are live. A stale initial handle has no neighbors.
Visited/result order, edge-filter calls, cancellation polling and budget accounting
remain at their existing locations. No binding, API or graph-creation change.

After promotion, membership needs one bit per arena slot. Bitmap initialization
uses at most one 64-bit word per node already discovered, so a sparse search on
a huge graph does not immediately allocate graph-sized storage. The initial hash
set and bitmap briefly coexist during promotion, then the hash table is released.

Graph/node construction, allocation controls, shallow reach, disconnected starts,
chains, hubs and random graphs were benchmarked. A single-insertion hash-set
candidate was rejected for inconsistent gains and DFS regressions. A node-id
`HashMap::entry` candidate was rejected because construction gains were
inconclusive and duplicate errors require another string allocation. Raw quick
trials, including noisy observations, are preserved separately from final samples.

## Build and runtime controls

Base: `7e7b7fa` (the existing environment's checkout). Existing Rust toolchain,
Cargo dependency cache, Python environment and the three current benchmark scripts
are reused. Historical report figures are not treated as measurements from this
run. No matching prior frozen paired artifacts or raw-sample harness was present.

Both artifacts use Rust 1.99.0, the same Cargo.lock, `--release --locked --offline`,
opt-level 3, default release LTO/codegen units and the same PyO3/Python environment.
This is the standard release profile, not the fat-LTO `dist` profile. Before and
after extensions and core benchmark binaries remain frozen under
`work/artifacts/{before,after}`. Input data files and all artifact SHA-256 hashes,
compiler details, CPU description, Python dependencies and thread settings are
in the raw-sample archive. Snapshot paths in its manifest describe the reused
cloud environment, not portable download locations.

All final runs use the same managed machine, CPU affinity 0–1,
RAYON_NUM_THREADS=2, OMP_NUM_THREADS=2, OPENBLAS_NUM_THREADS=2,
MKL_NUM_THREADS=2 and PYTHONHASHSEED=42. The machine exposes three logical CPUs
but has a two-CPU cgroup quota; final measurements use two CPUs. Builds, correctness
checks and benchmarks are strictly serialized. Exploratory runs with other
settings are labeled separately and never enter final estimates.

## Measurement protocol

A pair consists of fresh independent before and after processes, with AB/BA order
alternated across pairs. There are six focused pairs and three pairs for every
applicable full-suite workload. Each core process warms each operation and records
a 100-ms aggregate (at least three invocations), including graph destruction in
construction cases. These process aggregates are the independent raw samples;
inner invocations do not count as independent statistical samples.

The Python adapter retains all timed invocations, not minimum/best timings.
A pilot warms operations below one second, then samples for at least 120 ms,
at least three and at most 64 invocations. Operations whose pilot exceeds one
second retain that single measured call; independence comes from process pairs.
The same adaptive protocol is used for both variants. Setup stays outside timed
sections where the upstream harness excludes it. Cyclic GC is disabled within
each timed call. Python output deallocation is outside the timed call; core
construction includes drop. Inner timings and process means remain in raw JSONL.

General operations use `compare_networkx.run_size` with both default graph sizes
(1,000/5,000 and 20,000/100,000 random edges, plus the harness's connecting chain),
all operations, its result assertions and the 64-query loop control. NetworkX
reference calls run once per process and are not part of the Ironweaver estimate.
The analytics suite uses every operation from `compare_libraries.OPS`, each
default dataset (Facebook, GitHub, scale-16 Kronecker), and both one-off and reused
projection modes. Exact betweenness on GitHub and Kronecker exceeds the existing
5e9 nodes×edges budget and is skipped; Facebook exact betweenness is measured.
Memory/file scenarios use both default sizes (10,000/50,000 and 100,000/500,000),
fresh workers, matching immutable JSON input and the existing builders/save/load
functions. Peak-resident and file bytes are preserved alongside time samples.
Historical performance-result CSVs without executable benchmark sources are not
additional applicable current benchmarks.

## Statistics and regression handling

For each time workload, compute d_i = log(after_i / before_i) across process pairs.
Report exp(mean(d))−1 with the two-sided 95% Student-t interval
exp(mean(d) ± t(0.975, n−1) * sd(d)/sqrt(n))−1. Before/after times are arithmetic
means, so their quotient need not exactly equal the paired geometric effect.
Memory comparisons use paired byte differences and the corresponding Student-t
interval. Each interval is pointwise; it is not a family-wise guarantee across
all workloads. CIs overlapping zero are explicitly inconclusive. Identical file
sizes are marked unchanged.

Every primary interval indicating a regression triggers new independent pairs
using the same frozen artifacts, benchmark protocol, affinity and thread counts.
Repeat estimates are kept separate from primary estimates. The plots display the
independent repeat for flagged rows (†), while summary.csv retains both results.
A primary signal whose repeat overlaps zero remains unconfirmed/inconclusive; a
repeat indicating regression remains an explicit unresolved tradeoff or risk.
No noisy samples are dropped or overwritten. Aborted harness output is retained
under an `aborted-` name and never included in estimates. Only complete paired
processes with `.done` markers are analyzed.

## Correctness checks

- Python: 830 tests passed.
- Rust core: 148 tests and 3 doctests passed (151 total).
- cargo fmt --all --check: passed.
- cargo clippy --workspace --all-targets -- -D warnings: passed.
- Core dependency tree contains no PyO3.
- Existing general-operation comparisons/assertions run in the benchmark adapter.
- New core traversal coverage checks promotion, reused slot generations,
  stale roots, both directions, order, self-loops, parallel edges, filter calls,
  filter errors after promotion and truncating budgets.

The complete logs, frozen Cargo.lock, raw final and exploratory samples, metadata
and orchestration scripts are included in the archive. These results describe this
machine and these workloads; a broader production workload remains outside the
measurement scope.

Serialization peak RSS initially returned unavailable because the restricted
execution profile disallowed `/proc/self/clear_refs`. The runtime supports that
operation for a process's own VmHWM when the command receives the necessary
filesystem permission. Fresh independent before/after memory pairs use that
support with the same inputs, worker functions, machine, affinity and thread
settings. Null observations are retained, never replaced with inferred peaks.
These peak observations are identified as a late primary series in summary.csv;
existing resident/file samples remain the original primary series.

The core aggregate includes loop/clock bookkeeping. Sub-microsecond isolated and
shallow controls measure relative overhead under that harness, not an exact Python
API latency; the full general-operation suite measures the bindings' complete
call path separately.

Independent repeats use six fresh focused pairs, three general-operation pairs
for the 1,000-node size (both initial general signals occur there), and three
analytics pairs for the Kronecker dataset (the initial analytics signal occurs
there). Within each selected size/dataset, the same complete operation sequence,
setup, sampler, fixed seed, projection modes and runtime controls are retained.
The default full-suite selection remains unchanged. Repeat selection/settings
are included in `work/repeat-settings.json` in the archive. The primary and repeat
confidence intervals are not pooled.
