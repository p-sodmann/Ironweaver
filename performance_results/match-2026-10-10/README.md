# Matching: skip sorting when a label covers every node

The selected table row is `match_two_hops_100`. Its initial `Person`
candidate scan currently copies the label's entire hash set into a vector
and sorts it by slot. On the benchmark graph, every node is a `Person`, so
the graph's node iterator already produces exactly those candidates in the
same order. The matcher now streams that iterator when the rarest requested
label covers every node **and the arena has no deleted slots**. Sparse arenas
keep the label index; ID and property-index candidate selection still take
precedence. This avoids a whole-graph allocation and sort per scan.

## Benchmark scope

Baseline: core main `73d8fabbe2df56a9f9d07c70bf5a926f3366fd9a`, source tree
`0157f3934226084af6f95690c5152e0a953c0ff3`. The isolated baseline checkout
was verified to have exactly that source tree.

The input mirrors Ironweaver DB's seeded graph generator: 100k or 1M nodes,
four outgoing `KNOWS` edges per node, the same per-node xorshift64* RNG and
both random draws per edge, IDs `n{i}`, the `Person` label, node attributes
`age`, `group`, `x`, `y`, edge attribute `w`, and an index on `age`.
Each timed call parses `(a:Person {group: ...})-[:KNOWS]->(b)-[:KNOWS]->(c)`.

The DB enumerates matches before choosing its page. Accordingly the main
case uses its default work limits (100,000 visited nodes, 1,000,000 examined
edges, partial results allowed), **no core result limit**, and its visitor's
row conversion and max-heap selection of the first 100 rows by node IDs then
edge IDs. The maximum heap size is 101, just as in DB's `TopK`.

This is a **DB-shaped Rust kernel benchmark**, not the complete embedded
database operation. Graph construction/indexing are outside timing; query
parsing, matching, row conversion and top-100 selection are inside. Store
locks, worker dispatch, request handling and cursor construction are excluded.
The full DB build could not be run in this environment because the configured
network proxy was unavailable and its dependencies were not cached.

At 1M nodes the work limit is still 100,000 visits, matching the benchmark's
default request options. The larger gain reflects skipping a sort of all
1M candidates before a work-bounded search; it is not a claim about unlimited
enumeration of a 1M-node graph.

Source workload and query behavior:
- [DB graph generator](https://github.com/p-sodmann/ironweaver-db/blob/89bb1aacf36a62cdf3e4dfcb6fe2dff560e5ea68/crates/iwdb-server/benches/support/mod.rs)
- [DB benchmark](https://github.com/p-sodmann/ironweaver-db/blob/89bb1aacf36a62cdf3e4dfcb6fe2dff560e5ea68/crates/iwdb-server/benches/graph.rs)
- [Matching and pagination](https://github.com/p-sodmann/ironweaver-db/blob/89bb1aacf36a62cdf3e4dfcb6fe2dff560e5ea68/crates/iwdb-query/src/read/matching.rs)
- [Default work limits](https://github.com/p-sodmann/ironweaver-db/blob/89bb1aacf36a62cdf3e4dfcb6fe2dff560e5ea68/crates/iwdb-query/src/options.rs)

## Final measurements

AMD EPYC 9V74, Linux x86_64, Rust 1.99.0, Cargo release profile and default
compiler flags. One process at a time, pinned to CPU 0. No builds or tests
ran concurrently. The host exposes three CPUs with a two-CPU cgroup quota;
this sequential workload uses one CPU.

Four old/new/old process triplets per size. Each process warms up and
records five samples of at least 300 ms per case. Thus each entry is the
median of 40 before sample means or 20 after sample means. Within each
triplet, the drift-adjusted baseline is the average of the two old process
medians. `samples.json` retains every sample and verification signature;
`summary.json` retains medians and per-triplet improvements.

| Case | Nodes | Before | After | Median runtime reduction |
| --- | ---: | ---: | ---: | ---: |
| DB-shaped two hops, 100 rows | 100k | 31.076 ms | 29.464 ms | 5.2% |
| DB-shaped two hops, 100 rows | 1M | 54.964 ms | 35.456 ms | 35.5% |
| Core limit of 100 matches | 100k | 2.455 ms | 0.562 ms | 77.1% |
| Rare-label control (1% of nodes) | 100k | 74.940 µs | 77.326 µs | -3.2% |
| Unlabelled control | 100k | 28.977 ms | 29.459 ms | -1.7% |
| Sparse-arena control (100 live nodes) | 100k slots | 29.208 µs | 27.646 µs | 5.3% |

The 100k DB-shaped triplet gains are 12.7%, approximately 0%, 6.0% and 2.4%.
At 1M they are 36.0%, 32.9%, 36.0% and 39.7%. Controls show small gains and
regressions; this change does not establish an improvement for rare labels
or unlabelled queries. The bounded-core gain is not a prediction of the DB
row, because the DB does not cap core match enumeration at 100.

Every process verifies 32 groups spread across the 1,000 possible values.
Signatures include the complete ordered match stream, selected rows,
number of matches, visited nodes, examined edges and truncation. All
before/after signatures and counters match for every case and graph size.

## Correctness

All 164 Rust tests and six doc tests passed before and after. The new test
compares labelled and unlabelled ordered matches and exact budget counters
under work/result limits, with reused/deleted slots, parallel
edges, self-loops, group filters, property indexes and a rarer second label.
Existing tests compare matching with brute force and cover cancellation and
budget errors. Integration/lint results are recorded in `validation.txt`.

## Reproduce

From this PR checkout, freeze the same harness against both versions using
one toolchain, dependency lockfile and compiler flags:

```sh
cargo build -p ironweaver-core --example match_bench --release
cp target/release/examples/match_bench /tmp/iw-match-after
git worktree add --detach /tmp/iw-match-before 73d8fabbe2df56a9f9d07c70bf5a926f3366fd9a
cp crates/ironweaver-core/examples/match_bench.rs /tmp/iw-match-before/crates/ironweaver-core/examples/
cp Cargo.lock /tmp/iw-match-before/Cargo.lock
cargo build --manifest-path /tmp/iw-match-before/Cargo.toml -p ironweaver-core --example match_bench --release --target-dir /tmp/iw-match-before-target
python performance_results/match-2026-10-10/compare.py /tmp/iw-match-before-target/release/examples/match_bench /tmp/iw-match-after --cpu 0 --output /tmp/iw-match-samples.json
```

Omit `--cpu` on macOS. `--sizes` and `--triplets` can be overridden.

To measure the actual table row, point the DB's `ironweaver-core` dependency
at this PR's head, update its lockfile, and repeat the old/new/old run on
the same data and configuration:

```sh
IWDB_BENCH_NODES=100000 cargo bench -p iwdb-server --bench graph -- read/embedded/match_two_hops_100
```
