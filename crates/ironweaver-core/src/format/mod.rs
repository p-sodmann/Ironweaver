// format/mod.rs
//
// On-disk graph format, version 2: JSON (sonic-rs) or binary (postcard).
//
// Document shape (the same for both encodings):
//
//   { nodes:    { id: { id, labels, attr, meta, edge_ids, inverse_edge_ids } },
//     edges:    { edge_id: { id, from_id, to_id, type, attr, meta } },
//     meta:     { .. },
//     metadata: { version: "2.0", node_count, edge_count, timestamp,
//                 next_edge_id, indexes } }
//
// Edge ids are the `EdgeId`s in decimal; `next_edge_id` (a decimal string)
// keeps ids of removed edges from being reused after loading. `indexes`
// (only written if there are any) lists the property index paths, each a
// list of strings; loaders recreate the indexes (definitions only: the
// contents are rebuilt from the nodes). Readers that don't know it ignore
// it, and files without it load with no indexes. Every
// attribute value is an externally tagged `Value` (e.g. `{"Float": 1.5}`).
// JSON has no NaN or infinities: such a `Float` is written as one of the
// strings "NaN", "Infinity" and "-Infinity" (`Half`s are stored as their
// bits in both formats).
//
// Binary files are framed:
//
//   header  (16 bytes): b"IRONWEAV", u16 format version, u16 flags (0),
//                       u32 reserved (0); readers refuse unknown flags
//                       and a non-zero reserved field
//   payload:            the document, postcard-encoded
//   trailer (16 bytes): u64 payload length, u32 CRC32 of the payload,
//                       b"IWND"
//
// (little-endian). The trailer is written last, so the payload streams, and
// a truncated or corrupted file is detected before parsing.
//
// Version 1 JSON files (`metadata.version` "1.x", or none) still load.
// Version 1 binary files (headerless bincode) do not: they are recognised
// and refused with a message saying how to convert them (`unframed`). In
// version 1 JSON files, edge ids (strings like `edge_0_a_to_b`) are
// replaced by new `EdgeId`s (loaders can read the old one with
// `LoadEdge::id`), and the old conventions are migrated: a node
// attribute `labels` holding a list of strings becomes the node's labels, an
// edge attribute `type` holding a string becomes the edge's type.
//
// Saving streams the graph straight into the serializer (`GraphWriter`),
// asking a `Codec` to encode the payloads. Loading parses into borrowed
// structs (`LoadGraph`; strings point into the input buffer wherever
// possible) and `LoadGraph::build` turns them into a `Graph`, with payloads
// made by the caller. Binary files can also load from a reader
// (`LoadGraph::build_from_reader`, `stream.rs`), building the graph while
// decoding, so the file's bytes and the graph are never in memory at once.
// Attribute maps are written sorted by key, so equal graphs save to equal
// bytes (see `GraphWriter`).

mod load;
mod save;
mod stream;

/// The format version written.
pub const FORMAT_VERSION: &str = "2.0";

thread_local! {
    // First error message raised through `ser_error` / `de_error` since the
    // last `take_error` on this thread
    static LAST_ERROR: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

fn remember(msg: &str) {
    LAST_ERROR.with(|e| {
        let mut e = e.borrow_mut();
        if e.is_none() {
            *e = Some(msg.to_owned());
        }
    });
}

/// The message of the first error raised through [`ser_error`] /
/// [`de_error`] on this thread since the last call, if any; clears it.
///
/// The binary encoding (postcard) drops custom error messages, so a caller
/// that encodes values, ops or expressions with postcard itself can call
/// this before encoding (to clear a stale message) and after a failure (to
/// report the real reason, such as a depth limit).
pub fn take_error() -> Option<String> {
    LAST_ERROR.with(|e| e.borrow_mut().take())
}

/// A serde serialization error with `msg`. Use it for custom errors raised
/// while saving: the binary encoder (postcard) drops custom messages, so
/// `write_binary` reports the remembered one instead.
pub fn ser_error<E: serde::ser::Error>(msg: impl std::fmt::Display) -> E {
    let msg = msg.to_string();
    remember(&msg);
    E::custom(msg)
}

/// The same for errors raised while loading.
pub fn de_error<E: serde::de::Error>(msg: impl std::fmt::Display) -> E {
    let msg = msg.to_string();
    remember(&msg);
    E::custom(msg)
}

/// A postcard error, with the message of the custom error behind it.
fn postcard_error(e: postcard::Error) -> GraphError {
    GraphError::Format(take_error().unwrap_or_else(|| e.to_string()))
}

const MAGIC: &[u8; 8] = b"IRONWEAV";
const END: &[u8; 4] = b"IWND";
const HEADER_LEN: usize = 16;
const TRAILER_LEN: usize = 16;

fn binary_header() -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    h[..8].copy_from_slice(MAGIC);
    h[8..10].copy_from_slice(&2u16.to_le_bytes());
    h
}

fn binary_trailer(len: u64, crc: u32) -> [u8; TRAILER_LEN] {
    let mut t = [0u8; TRAILER_LEN];
    t[..8].copy_from_slice(&len.to_le_bytes());
    t[8..12].copy_from_slice(&crc.to_le_bytes());
    t[12..].copy_from_slice(END);
    t
}

fn bad_binary(what: &str) -> GraphError {
    GraphError::Format(format!("invalid ironweaver binary file: {}", what))
}

/// Header flags this reader understands (none so far). A flag changes how a
/// file is read, so a file with an unknown one is refused, not misread.
const KNOWN_FLAGS: u16 = 0;

/// Check the version, flags and reserved field in a binary file's header.
fn check_version(header: &[u8]) -> Result<(), GraphError> {
    let version = u16::from_le_bytes([header[8], header[9]]);
    if version != 2 {
        return Err(GraphError::Format(format!(
            "binary format version {} is not supported (written by a newer ironweaver?)",
            version
        )));
    }
    let flags = u16::from_le_bytes([header[10], header[11]]);
    if flags & !KNOWN_FLAGS != 0 {
        return Err(GraphError::Format(format!(
            "binary header flags {:#06x} are not supported (written by a newer ironweaver?)",
            flags
        )));
    }
    if header[12..16] != [0; 4] {
        return Err(bad_binary("reserved header bytes are not zero (corrupted)"));
    }
    Ok(())
}

/// Check a binary file's trailer against the payload's length and CRC32.
fn check_trailer(trailer: &[u8], len: u64, crc: u32) -> Result<(), GraphError> {
    if &trailer[12..] != END {
        return Err(bad_binary("truncated (no trailer)"));
    }
    if u64::from_le_bytes(trailer[..8].try_into().expect("8 bytes")) != len {
        return Err(bad_binary("length mismatch (truncated?)"));
    }
    if u32::from_le_bytes(trailer[8..12].try_into().expect("4 bytes")) != crc {
        return Err(bad_binary("checksum mismatch (corrupted)"));
    }
    Ok(())
}

/// The postcard payload of a framed binary file (after checking header,
/// length and checksum).
fn binary_payload(bytes: &[u8]) -> Result<&[u8], GraphError> {
    if !bytes.starts_with(MAGIC) {
        return Err(unframed(bytes));
    }
    if bytes.len() < HEADER_LEN + TRAILER_LEN {
        return Err(bad_binary("truncated"));
    }
    check_version(&bytes[..HEADER_LEN])?;
    let (body, trailer) = bytes[HEADER_LEN..].split_at(bytes.len() - HEADER_LEN - TRAILER_LEN);
    check_trailer(trailer, body.len() as u64, crc32fast::hash(body))?;
    Ok(body)
}

/// The error for a file that doesn't start with the binary header, given
/// its first bytes (at least `HEADER_LEN` of them if the file has that
/// many). A version 1 binary file (headerless bincode, ironweaver 0.1)
/// gets its own message: it starts with the node count and then the first
/// node id's length (or the edge count) as little-endian u64s, which
/// leaves the high halves of both zero.
fn unframed(start: &[u8]) -> GraphError {
    if MAGIC.starts_with(start) {
        return bad_binary("truncated");
    }
    let v1 = start.len() >= HEADER_LEN && start[4..8] == [0; 4] && start[12..16] == [0; 4];
    if v1 {
        GraphError::Format(
            "unsupported ironweaver binary format version 1 (saved by ironweaver 0.1): this version reads \
             binary format version 2 and newer. To convert the file, load it with ironweaver 0.1, save it \
             with save_to_json, and load that JSON file with this version"
                .into(),
        )
    } else {
        bad_binary("no IRONWEAV header (not an ironweaver binary graph file)")
    }
}

pub use load::{LoadAttrs, LoadEdge, LoadGraph, LoadKind, LoadNode, LoadValue, MAX_DEPTH};
pub use save::{tagged, Codec, GraphWriter, RecordCodec};

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, BufWriter};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{Attrs, Graph, GraphError, Record};

/// Encode a `Graph<Record, Record>` (with graph-level `meta`) as JSON.
pub fn to_json(graph: &Graph<Record, Record>, meta: &Attrs, pretty: bool) -> Result<Vec<u8>, GraphError> {
    GraphWriter::new(graph, &RecordCodec { meta, half: false }).to_json(pretty)
}

/// Encode a `Graph<Record, Record>` (with graph-level `meta`) as a binary
/// file of format version 2: header, postcard payload and a trailer with
/// length and CRC32. `half` stores floats at half precision.
pub fn to_binary(graph: &Graph<Record, Record>, meta: &Attrs, half: bool) -> Result<Vec<u8>, GraphError> {
    let mut out = Vec::new();
    GraphWriter::new(graph, &RecordCodec { meta, half }).write_binary(&mut out)?;
    Ok(out)
}

/// Decode a JSON document into a `Graph<Record, Record>` and its graph-level meta.
pub fn from_json(bytes: &[u8]) -> Result<(Graph<Record, Record>, Attrs), GraphError> {
    records(&LoadGraph::from_json_slice(bytes)?)
}

/// Decode a binary document into a `Graph<Record, Record>` and its
/// graph-level meta: a framed postcard file (format version 2). Version 1
/// binary files are a [`GraphError::Format`] saying how to convert them.
pub fn from_binary(bytes: &[u8]) -> Result<(Graph<Record, Record>, Attrs), GraphError> {
    records(&LoadGraph::from_binary_slice(bytes)?)
}

/// Decode a binary document from a reader into a `Graph<Record, Record>`
/// and its graph-level meta, without holding the whole file in memory
/// (see [`LoadGraph::build_from_reader`]).
pub fn from_binary_reader(reader: impl std::io::Read) -> Result<(Graph<Record, Record>, Attrs), GraphError> {
    let (mut graph, meta) = LoadGraph::build_from_reader(
        reader,
        |n| Ok::<_, GraphError>(Record { attr: n.attr().to_attrs(), meta: n.meta().to_attrs() }),
        |e| Ok(Record { attr: e.attr().to_attrs(), meta: e.meta().to_attrs() }),
    )?;
    graph.flush_indexes()?;
    Ok((graph, meta.to_attrs()))
}

fn records(doc: &LoadGraph<'_>) -> Result<(Graph<Record, Record>, Attrs), GraphError> {
    let mut graph = doc.build(
        |n| Ok::<_, GraphError>(Record { attr: n.attr().to_attrs(), meta: n.meta().to_attrs() }),
        |e| Ok(Record { attr: e.attr().to_attrs(), meta: e.meta().to_attrs() }),
    )?;
    graph.flush_indexes()?;
    Ok((graph, doc.meta().to_attrs()))
}

/// Write the file at `path` through `write`, atomically: the data goes to a
/// temporary file next to it, which is flushed to disk and then renamed over
/// `path`. A failed or interrupted save leaves any previous file untouched
/// (and removes the temporary file when it can). An existing file's
/// permissions are kept; a symlink at `path` is replaced, not written through.
///
/// `Ok` means the new contents are on disk (fsynced), the rename is done,
/// and, on Unix, the rename itself is durable: the directory holding `path`
/// was fsynced too, so after a crash or power loss `path` holds the new
/// file. Windows has no way to sync a directory; there the rename is
/// done but not explicitly made durable.
///
/// If only that last step fails (the directory can't be opened or its fsync
/// fails), the error is returned although `path` already names the new
/// file: it may still revert to the old one after a crash. A failed fsync
/// can't be made up for by syncing again, so callers that need durability
/// should treat the save as failed.
pub fn write_atomic(
    path: impl AsRef<Path>,
    write: impl FnOnce(&mut BufWriter<File>) -> io::Result<()>,
) -> io::Result<()> {
    // Distinguishes concurrent saves from one process
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let path = path.as_ref();
    let name = path.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a file path"))?;
    let mut tmp_name = OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(format!(".{}.{}.tmp", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed)));
    let tmp = path.with_file_name(tmp_name);

    let result = (|| {
        let mut out = BufWriter::new(File::create(&tmp)?);
        write(&mut out)?;
        let file = out.into_inner().map_err(io::IntoInnerError::into_error)?;
        if let Ok(meta) = fs::metadata(path) {
            file.set_permissions(meta.permissions())?;
        }
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
        return result;
    }
    // Make the rename itself durable (not possible on Windows)
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        let dir = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
        File::open(dir).and_then(|d| d.sync_all()).map_err(|e| {
            io::Error::new(e.kind(), format!("saved, but syncing the directory {} failed: {e}", dir.display()))
        })?;
    }
    Ok(())
}
