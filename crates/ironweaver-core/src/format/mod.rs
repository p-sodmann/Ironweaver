// format/mod.rs
//
// On-disk graph format (JSON via sonic-rs, binary via bincode).
//
// Document shape (the same for both encodings):
//
//   { nodes:    { id: { id, attr, meta, edge_ids, inverse_edge_ids } },
//     edges:    { edge_id: { id, from_id, to_id, attr, meta } },
//     meta:     { .. },
//     metadata: { version, node_count, edge_count, timestamp } }
//
// Every attribute value is encoded as an externally tagged `Value` (e.g.
// `{"Float": 1.5}`); files written by older versions still load.
//
// Saving streams the graph straight into the serializer (`GraphWriter`),
// asking a `Codec` to encode the payloads. Loading parses into borrowed
// structs (`LoadGraph`; strings point into the input buffer wherever
// possible) and `LoadGraph::build` turns them into a `Graph`, with payloads
// made by the caller.

mod load;
mod save;

pub use load::{LoadAttrs, LoadEdge, LoadGraph, LoadKind, LoadNode, LoadValue};
pub use save::{tagged, Codec, GraphWriter, RecordCodec};

use crate::{Attrs, Graph, GraphError, Record};

/// Encode a `Graph<Record, Record>` (with graph-level `meta`) as JSON.
pub fn to_json(graph: &Graph<Record, Record>, meta: &Attrs, pretty: bool) -> Result<Vec<u8>, GraphError> {
    GraphWriter::new(graph, &RecordCodec { meta, half: false }).to_json(pretty)
}

/// Encode a `Graph<Record, Record>` with bincode; `half` stores floats at
/// half precision.
pub fn to_binary(graph: &Graph<Record, Record>, meta: &Attrs, half: bool) -> Result<Vec<u8>, GraphError> {
    let mut out = Vec::new();
    GraphWriter::new(graph, &RecordCodec { meta, half }).write_binary(&mut out)?;
    Ok(out)
}

/// Decode a JSON document into a `Graph<Record, Record>` and its graph-level meta.
pub fn from_json(bytes: &[u8]) -> Result<(Graph<Record, Record>, Attrs), GraphError> {
    records(&LoadGraph::from_json_slice(bytes)?)
}

/// Decode a bincode document into a `Graph<Record, Record>` and its graph-level meta.
pub fn from_binary(bytes: &[u8]) -> Result<(Graph<Record, Record>, Attrs), GraphError> {
    records(&LoadGraph::from_binary_slice(bytes)?)
}

fn records(doc: &LoadGraph<'_>) -> Result<(Graph<Record, Record>, Attrs), GraphError> {
    let graph = doc.build(
        |n| Ok::<_, GraphError>(Record { attr: n.attr().to_attrs(), meta: n.meta().to_attrs() }),
        |e| Ok(Record { attr: e.attr().to_attrs(), meta: e.meta().to_attrs() }),
    )?;
    Ok((graph, doc.meta().to_attrs()))
}
