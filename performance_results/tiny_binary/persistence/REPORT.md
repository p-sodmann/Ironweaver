# Durable BLOB persistence integration comparison

Storage reference: SQLite WAL, `synchronous=FULL`, 4 KiB pages, under `/workspace` (overlay filesystem), not `/tmp` (tmpfs). This is not an Ironweaver DB benchmark or a physical-drive durability certification. SQLite is part of the Python standard library.

The integration test verifies raw/gzip/LZ4/Zstd graph snapshots after closing and reopening the database, including metadata, indexes, edges, independent users, updates and failed-batch rollback. CI has no timing assertions. Compression packages are test/benchmark-only dependencies.

## Method

Three independent process pairs alternate streamed-CRC and batched-CRC optimized artifacts. Each process runs 15 measured commits per case; codec order alternates each round. Raw samples and extension hashes are retained. This is a workload experiment, not a production p95 estimate.

Every write includes graph encoding, optional compression and the synchronous WAL commit. Business mutations happen before the timer. Encoding happens before beginning the transaction to shorten writer-lock occupancy. Each write is followed by SELECT, decompression and graph reconstruction; destruction of the returned graphs is outside read timing. Final reopen/read-back and WAL checkpoint are verified outside the timer. WAL auto-checkpoints are disabled during measurement.

Each graph has 1, 10 or 100 nodes, two edges per node, three node attributes, labels, edge types and weights. Each commit replaces one or 100 user records. Cases update one score or all node scores, avoiding a benchmark that merely changes the save timestamp. The 100-record batch repeats the same graph content under independent user keys; all codecs see the same input values. Reused compression contexts are used sequentially.

## Candidate results

Values are medians of the three process medians. Times are per complete commit/read batch, not per graph. WAL growth is logical file growth per commit, not measured device bytes. Database size is measured after a checkpoint.

### single_attribute

| Nodes/graph | Graphs/commit | Codec | Commit µs | Read/restore µs | DB bytes | WAL growth bytes/commit |
| ---: | ---: | --- | ---: | ---: | ---: | ---: |
| 1 | 1 | raw | 445.4 | 30.4 | 12288 | 4120 |
| 1 | 1 | lz4 | 423.2 | 32.0 | 12288 | 4120 |
| 1 | 1 | zstd | 495.1 | 38.7 | 12288 | 4120 |
| 1 | 1 | gzip | 514.6 | 44.7 | 12288 | 4120 |
| 1 | 100 | raw | 703.2 | 813.6 | 45056 | 32960 |
| 1 | 100 | lz4 | 785.5 | 856.6 | 40960 | 28840 |
| 1 | 100 | zstd | 1228.4 | 1049.5 | 36864 | 24720 |
| 1 | 100 | gzip | 1910.0 | 1124.4 | 36864 | 24720 |
| 10 | 1 | raw | 438.2 | 47.7 | 12288 | 4120 |
| 10 | 1 | lz4 | 452.3 | 48.9 | 12288 | 4120 |
| 10 | 1 | zstd | 418.5 | 54.8 | 12288 | 4120 |
| 10 | 1 | gzip | 435.5 | 68.0 | 12288 | 4120 |
| 10 | 100 | raw | 1791.5 | 3135.3 | 217088 | 206000 |
| 10 | 100 | lz4 | 1779.9 | 3065.6 | 94208 | 82400 |
| 10 | 100 | zstd | 2257.1 | 3415.7 | 73728 | 61800 |
| 10 | 100 | gzip | 3540.8 | 4160.7 | 73728 | 61800 |
| 100 | 1 | raw | 485.0 | 205.4 | 24576 | 8240 |
| 100 | 1 | lz4 | 495.8 | 238.2 | 20480 | 8240 |
| 100 | 1 | zstd | 546.5 | 220.6 | 12288 | 4120 |
| 100 | 1 | gzip | 679.5 | 256.7 | 12288 | 4120 |
| 100 | 100 | raw | 8855.9 | 25060.2 | 1662976 | 824000 |
| 100 | 100 | lz4 | 9857.0 | 25364.5 | 565248 | 560320 |
| 100 | 100 | zstd | 11546.6 | 27300.2 | 421888 | 412000 |
| 100 | 100 | gzip | 15321.0 | 28717.1 | 421888 | 412000 |
### all_attributes

| Nodes/graph | Graphs/commit | Codec | Commit µs | Read/restore µs | DB bytes | WAL growth bytes/commit |
| ---: | ---: | --- | ---: | ---: | ---: | ---: |
| 1 | 1 | raw | 428.4 | 27.6 | 12288 | 4120 |
| 1 | 1 | lz4 | 432.2 | 31.1 | 12288 | 4120 |
| 1 | 1 | zstd | 466.5 | 33.9 | 12288 | 4120 |
| 1 | 1 | gzip | 435.0 | 39.3 | 12288 | 4120 |
| 1 | 100 | raw | 746.3 | 796.3 | 45056 | 32960 |
| 1 | 100 | lz4 | 788.4 | 931.5 | 40960 | 28840 |
| 1 | 100 | zstd | 1335.6 | 1095.0 | 36864 | 24720 |
| 1 | 100 | gzip | 1901.6 | 1275.1 | 36864 | 24720 |
| 10 | 1 | raw | 368.2 | 46.3 | 12288 | 4120 |
| 10 | 1 | lz4 | 392.6 | 48.7 | 12288 | 4120 |
| 10 | 1 | zstd | 384.1 | 51.6 | 12288 | 4120 |
| 10 | 1 | gzip | 418.7 | 63.3 | 12288 | 4120 |
| 10 | 100 | raw | 1666.2 | 3035.1 | 217088 | 206000 |
| 10 | 100 | lz4 | 1760.4 | 3095.3 | 94208 | 82400 |
| 10 | 100 | zstd | 2230.7 | 3485.4 | 73728 | 61800 |
| 10 | 100 | gzip | 3383.6 | 3996.7 | 73728 | 61800 |
| 100 | 1 | raw | 490.2 | 220.8 | 24576 | 16480 |
| 100 | 1 | lz4 | 472.4 | 216.3 | 20480 | 16480 |
| 100 | 1 | zstd | 589.0 | 225.7 | 12288 | 4120 |
| 100 | 1 | gzip | 633.4 | 259.1 | 12288 | 4120 |
| 100 | 100 | raw | 9587.0 | 26097.3 | 1662976 | 1648000 |
| 100 | 100 | lz4 | 9409.0 | 26625.0 | 565248 | 560320 |
| 100 | 100 | zstd | 11600.0 | 28043.4 | 421888 | 412000 |
| 100 | 100 | gzip | 15463.9 | 30825.2 | 421888 | 412000 |

## Encoder change, raw blobs

The baseline is the bytes-API prototype using per-write CRC updates; the candidate hashes the completed buffer once. The same binary format is used. The in-memory report separately isolates this change. These durable wall times mix encoding with filesystem scheduling and sync latency; differences beyond the measured CPU savings cannot simply be attributed to the encoder.

| Update | Nodes/graph | Graphs/commit | Streamed-CRC commit µs | Batched-CRC commit µs |
| --- | ---: | ---: | ---: | ---: |
| single_attribute | 1 | 1 | 393.3 | 445.4 |
| single_attribute | 1 | 100 | 804.8 | 703.2 |
| single_attribute | 10 | 1 | 412.8 | 438.2 |
| single_attribute | 10 | 100 | 2203.4 | 1791.5 |
| single_attribute | 100 | 1 | 580.4 | 485.0 |
| single_attribute | 100 | 100 | 13714.2 | 8855.9 |
| all_attributes | 1 | 1 | 343.7 | 428.4 |
| all_attributes | 1 | 100 | 812.3 | 746.3 |
| all_attributes | 10 | 1 | 367.6 | 368.2 |
| all_attributes | 10 | 100 | 2259.2 | 1666.2 |
| all_attributes | 100 | 1 | 491.3 | 490.2 |
| all_attributes | 100 | 100 | 15241.2 | 9587.0 |

## Interpretation

- Tiny individual snapshots often occupy the same SQLite page whether compressed or raw. Smaller payloads alone do not eliminate the commit sync or guarantee lower latency.
- Compression reduces checkpoint size and can reduce WAL traffic for packed batches. WAL savings depend on which attributes change and which pages become dirty; payload size is not a substitute for measuring WAL writes.
- Compression CPU is included. Compare each workload in the tables; there is no justification here for enabling one codec unconditionally. Raw bytes remain the latency baseline, while LZ4/Zstd offer configurable space tradeoffs.
- Batching amortizes fixed commit costs across graphs, at the cost of a different transaction boundary.
- The filesystem and SQLite cache remain warm. No cold-cache reset, concurrent clients, network transport, IWDB WAL/transactions or crash-injection test is included. Deployment-specific storage tests remain necessary.

## Reproduction

```bash
python benchmarks/persist_binary.py --directory /path/on/target/filesystem --output results.json
pytest tests/test_binary_persistence.py
```

Use optimized builds and install the test extras (including optional compression packages). Run one benchmark process at a time. Earlier atomic-file timings used `/tmp` tmpfs; this integration report supersedes them for storage-cost comparisons.
