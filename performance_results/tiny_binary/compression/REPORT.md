# Compression of tiny binary graphs

CPython 3.13.5, optimized binary bytes encoder, two independent process runs. Each process takes five repeat samples. Optional packages: zstandard 0.25.0, lz4 4.4.5. Exact byte round trips and graph reconstruction are checked. Raw observations are retained.

Compression runs after binary encoding; decompress before `Vertex.from_binary`. Gzip and zlib wrap DEFLATE with different framing. Raw DEFLATE omits that wrapper. The inner graph format/checksum remains intact.

## Ten nodes / twenty edges

| Codec | Bytes | Compress only µs | Graph encode + compress µs | Decompress + graph load µs |
| --- | ---: | ---: | ---: | ---: |
| none | 1449 | 0.04 | 6.16 | 22.54 |
| lz4_frame | 699 | 1.58 | 7.78 | 24.09 |
| zstd_1_reused | 507 | 6.64 | 12.99 | 27.56 |
| zstd_1_fresh | 507 | 15.79 | 22.55 | 37.70 |
| gzip_1 | 524 | 11.76 | 24.65 | 29.66 |
| zlib_1 | 512 | 12.49 | 24.41 | 29.05 |
| raw_deflate_1 | 506 | 11.27 | 22.93 | 27.42 |

Medians of process medians, without confidence intervals. Separate stage medians are not additive. Save timestamps change between encodes, so compression work and sizes vary slightly.

## Payload dependence

| Payload | Raw bytes | LZ4 bytes | Zstd 1 bytes | Gzip 1 bytes |
| --- | ---: | ---: | ---: | ---: |
| 0 nodes, scalar | 138 | 146 | 132 | 135 |
| 1 nodes, scalar | 265 | 240 | 210 | 214 |
| 10 nodes, scalar | 1449 | 699 | 507 | 524 |
| 100 nodes, scalar | 14633 | 5295 | 3022 | 3553 |
| 1 nodes, embedding | 3734 | 3735 | 3456 | 3397 |
| 10 nodes, embedding | 36139 | 35363 | 32199 | 31466 |

Scalar nodes have a name, score, short tags list and label; each has two typed outgoing edges with a weight. Embedding nodes additionally own 384 deterministic pseudo-random floats, avoiding repeated constant vectors.

## Recommendation

- Keep raw bytes for minimum CPU latency and poorly compressible blobs.
- LZ4 is a candidate for low-overhead size reduction on repetitive tiny graphs; Zstd level 1 saves more space in this scalar workload.
- Gzip is useful for HTTP interoperability but costs more CPU here. Higher DEFLATE levels brought little benefit for random vectors.
- For adaptive selection, store the codec/version beside the blob and keep raw bytes when compression does not save enough space.
- Reused Zstd contexts exclude setup cost; fresh contexts are measured too. Use a separate context per simultaneous worker/thread or serialize access.
- A trained Zstd dictionary is another candidate for many similar tiny graphs, requiring representative training and versioned dictionary distribution.

These tests exclude storage commits, fsync, network transfer, HTTP overhead and browser work. Compression adds CPU; reduced I/O may outweigh it for the complete application. No automatic compression or new compressed graph format was added to the library.

Reproduce with `python benchmarks/compare_binary_compression.py`. Zstandard and LZ4 are optional benchmark-only packages, not runtime dependencies.
