# File format and compatibility

`save_to_json`, `save_to_binary` and `save_to_binary_f16` write the whole graph: nodes (ids, labels, `attr`, `meta`), edges (ids, endpoints, types, `attr`, `meta`), the graph's `meta`, and the next edge id. `load_from_json` / `load_from_binary` read it back.

```python
import os, tempfile
from ironweaver import Vertex

g = Vertex()
g.add_node("a", {"age": 3}, labels=["Person"])
g.add_node("b")
g.add_edge("a", "b", {"w": 1.5}, type="knows")
path = os.path.join(tempfile.mkdtemp(), "graph.bin")
g.save_to_binary(path)
h = Vertex.load_from_binary(path)
assert h["a"].labels == ["Person"] and h["a"].edges[0].type == "knows"
assert h.add_edge("b", "a").id == 1        # edge ids continue where they left off
```

## Compatibility promise

- **ironweaver reads every format version it has written.** Files saved by 0.1 (format 1) load in 0.2 and later.
- **A new format version only comes with a minor release** (0.2 → 0.3, or 1.x → 1.y), and the changelog says so. Patch releases never change what is written.
- **Older releases can't read newer files.** 0.1 can't read format 2. Keep the old version around (or save to JSON with it) if you need to go back.
- The test suite loads saved files from every format version (`tests/data/legacy_*` for format 1, `tests/data/v2_*` for format 2), so these guarantees are checked on every change.

## Format 2 (ironweaver 0.2)

Both encodings hold the same document:

```text
{ nodes:    { id: { id, labels, attr, meta, edge_ids, inverse_edge_ids } },
  edges:    { edge_id: { id, from_id, to_id, type, attr, meta } },
  meta:     { ... },
  metadata: { version: "2.0", node_count, edge_count, timestamp, next_edge_id, indexes } }
```

- Edge ids are the persistent integer ids, written in decimal. `next_edge_id` keeps the ids of removed edges from being handed out again after loading.
- Attribute values are tagged with their type (`{"Float": 1.5}`, `{"String": "x"}`, ...), so ints, floats, strings, bools, None, lists and dicts all round-trip exactly, and so do:
  - bytes (`bytes` / `bytearray`; loaded as `bytes`): base64 in JSON, `{"Bytes": "aGk="}`;
  - dates (`datetime.date`): `{"Date": "2024-05-01"}`;
  - date-times (`datetime.datetime`): `{"DateTime": "2024-05-01T12:30:00.250000+02:00"}`, microsecond precision. An aware datetime keeps its UTC offset but not its zone name (it loads with a fixed-offset `timezone`); a naive one loads naive.

  - floats exactly, `-0.0` included. JSON has no literals for NaN and the infinities, so they are written as strings: `{"Float": "NaN"}`, `{"Float": "Infinity"}`, `{"Float": "-Infinity"}` (a NaN loads as the standard NaN).

  In binary files dates are day counts, date-times microseconds plus offset, bytes raw. Files saved before 0.2 stored datetimes as strings (and bytes as lists of ints); they load as they were saved.
- `indexes` lists the indexed attribute paths (each a list of strings), and is only written when there are indexes. Loading recreates them from the nodes; the index contents aren't stored. Files without it load with no indexes, and readers that don't know it ignore it.
- Values may nest at most 100 levels deep; deeper files are rejected when loading, so a crafted file can't exhaust the stack.

**JSON** is the document above, compact by default (`save_to_json(path, pretty=True)` indents it).

**Binary** is framed so that a truncated or corrupted file is detected before anything is parsed:

```text
header   16 bytes   b"IRONWEAV", u16 format version (2), u16 flags (0), u32 reserved (0)
payload             the document, postcard-encoded
trailer  16 bytes   u64 payload length, u32 CRC32 of the payload, b"IWND"
```

All integers are little-endian. No flags are defined yet: a reader refuses a file with a flag it doesn't know (a newer writer may use one to change how the file is read) and a file whose reserved field isn't 0, since the checksum covers only the payload. `save_to_binary_f16` stores floats (and lists of floats) at half precision, for embeddings where the saving matters more than the precision.

Every save writes to a temporary file, flushes it to disk and then renames it over the target, so a crash mid-save never leaves a half-written file behind.

Saves are deterministic: attribute maps are written sorted by key, so two graphs with the same contents and the same node and edge order save to the same bytes, except for `metadata.timestamp` (the time of the save). In Rust, `GraphWriter::with_timestamp` fixes or leaves out the timestamp for byte-identical files.

Rust programs can also load a binary file from a reader (`format::from_binary_reader`, `LoadGraph::build_from_reader`): the graph is built while the payload is decoded, so the file's bytes are never all in memory next to the graph. The checksum is still checked, at the end; a file that fails it is rejected as a whole.

## Format 1 (ironweaver 0.1)

Format 1 files (JSON with `metadata.version` "1.x", or binary files without the `IRONWEAV` header) are converted while loading:

- a node attribute `labels` holding a list of strings becomes the node's labels, and an edge attribute `type` holding a string becomes the edge's type;
- edges get new integer ids; the old string id (like `edge_0_a_to_b`) is kept in `edge.meta["legacy_id"]`.

Saving the loaded graph again writes format 2.
