# Tiny graph binary serialization

CPython 3.13.5, optimized Rust release builds; three independent process pairs
with alternating artifact order. Each process takes seven repeat samples.
Graphs contain three node attributes, one label, two edges per node, an edge
attribute and an edge type. Raw samples and extension hashes are retained.

| Nodes / edges | Streamed CRC encoding µs | Batched CRC encoding µs | Encoding speedup | Bytes decoding µs | Atomic file save µs |
| --- | ---: | ---: | ---: | ---: | ---: |
| 0 / 0 | 0.74 | 0.45 | 1.66× | 1.12 | 8.30 |
| 1 / 2 | 2.01 | 1.14 | 1.77× | 3.59 | 9.54 |
| 10 / 20 | 11.25 | 5.66 | 1.99× | 21.68 | 19.80 |
| 100 / 200 | 97.28 | 50.30 | 1.93× | 199.09 | 118.78 |

The streamed-CRC baseline is the initial in-memory prototype using the existing
`write_binary` path, not a previously released Python bytes API. The candidate
uses Postcard directly into a Vec and calculates CRC32 over the completed
payload once. The same header, payload encoding and checksum/trailer are kept.
Core tests compare both paths byte-for-byte with a fixed/omitted timestamp,
including f16 and deeply nested payloads.

`Vertex.to_binary()` returns bytes; `Vertex.from_binary(blob)` reads immutable
bytes without file I/O. The existing file methods keep their atomic writes and
fsync/rename behavior and use the streaming writer. File timings come from this
`/tmp` tmpfs (RAM-backed storage), not physical disk; they are not estimates of
production storage latency. The persistence integration benchmark separately
measures SQLite commits on the selected filesystem.
Encoding bytes is not durable storage; the database/blob layer still needs to
commit them. This does not measure DB commits, WAL behavior, RPC or HTTP.

The reusable benchmark is `benchmarks/compare_binary_bytes.py`. Run it against
an optimized build; the new bytes APIs are required. Existing binary framing,
precision, validation and file compatibility remain unchanged.
