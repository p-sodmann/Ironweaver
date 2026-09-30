// format/save.rs

use serde::ser::{SerializeMap, SerializeSeq, SerializeStruct, Serializer};
use serde::Serialize;
use std::io::Write;

use crate::{Attrs, Edge, Graph, GraphError, Node, Record, Value};

/// Encoders for the tagged `Value` representation, for codecs that encode
/// their own value types without building `Value`s first.
pub mod tagged {
    use half::f16;
    use serde::{Serialize, Serializer};

    const ENUM: &str = "Value";
    // Variant indices of `Value` (part of the binary format)
    const STRING: u32 = 0;
    const INT: u32 = 1;
    const FLOAT: u32 = 2;
    const HALF: u32 = 3;
    const BOOL: u32 = 4;
    const NONE: u32 = 5;
    const LIST: u32 = 6;
    const DICT: u32 = 7;
    const BYTES: u32 = 8;
    const DATE: u32 = 9;
    const DATETIME: u32 = 10;

    pub fn string<S: Serializer>(s: S, v: &str) -> Result<S::Ok, S::Error> {
        s.serialize_newtype_variant(ENUM, STRING, "String", v)
    }

    pub fn int<S: Serializer>(s: S, v: i64) -> Result<S::Ok, S::Error> {
        s.serialize_newtype_variant(ENUM, INT, "Int", &v)
    }

    /// A float; at half precision if `half`.
    pub fn float<S: Serializer>(s: S, v: f64, half: bool) -> Result<S::Ok, S::Error> {
        if half {
            s.serialize_newtype_variant(ENUM, HALF, "Half", &f16::from_f64(v))
        } else {
            s.serialize_newtype_variant(ENUM, FLOAT, "Float", &v)
        }
    }

    pub fn half<S: Serializer>(s: S, v: f16) -> Result<S::Ok, S::Error> {
        s.serialize_newtype_variant(ENUM, HALF, "Half", &v)
    }

    pub fn bool<S: Serializer>(s: S, v: bool) -> Result<S::Ok, S::Error> {
        s.serialize_newtype_variant(ENUM, BOOL, "Bool", &v)
    }

    pub fn none<S: Serializer>(s: S) -> Result<S::Ok, S::Error> {
        s.serialize_unit_variant(ENUM, NONE, "None")
    }

    /// A list; `items` must serialize as a sequence of tagged values.
    pub fn list<S: Serializer, T: Serialize + ?Sized>(s: S, items: &T) -> Result<S::Ok, S::Error> {
        s.serialize_newtype_variant(ENUM, LIST, "List", items)
    }

    /// A dict; `entries` must serialize as a map of string keys to tagged values.
    pub fn dict<S: Serializer, T: Serialize + ?Sized>(s: S, entries: &T) -> Result<S::Ok, S::Error> {
        s.serialize_newtype_variant(ENUM, DICT, "Dict", entries)
    }

    /// A byte string (base64 in text formats).
    pub fn bytes<S: Serializer>(s: S, v: &[u8]) -> Result<S::Ok, S::Error> {
        struct Bytes<'a>(&'a [u8]);
        impl Serialize for Bytes<'_> {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                crate::temporal::bytes::serialize(self.0, s)
            }
        }
        s.serialize_newtype_variant(ENUM, BYTES, "Bytes", &Bytes(v))
    }

    pub fn date<S: Serializer>(s: S, v: crate::Date) -> Result<S::Ok, S::Error> {
        s.serialize_newtype_variant(ENUM, DATE, "Date", &v)
    }

    pub fn datetime<S: Serializer>(s: S, v: crate::DateTime) -> Result<S::Ok, S::Error> {
        s.serialize_newtype_variant(ENUM, DATETIME, "DateTime", &v)
    }

    /// Fails when `depth` (1 for an attribute's own value) exceeds
    /// [`MAX_DEPTH`](crate::format::MAX_DEPTH), as the loaders would reject
    /// the value. Also stops values that contain themselves.
    pub fn check_depth<E: serde::ser::Error>(depth: usize) -> Result<(), E> {
        let max = crate::format::MAX_DEPTH;
        if depth > max {
            return Err(crate::format::ser_error(format_args!(
                "attribute values nested more than {max} levels deep (or containing themselves) cannot be saved"
            )));
        }
        Ok(())
    }
}

/// Encodes the payloads of a `Graph<N, E>` while it is written. Every method
/// must write a map of string keys to tagged values (see [`tagged`]).
pub trait Codec<N, E> {
    fn node_attr<S: Serializer>(&self, node: &N, s: S) -> Result<S::Ok, S::Error>;
    fn node_meta<S: Serializer>(&self, node: &N, s: S) -> Result<S::Ok, S::Error>;
    fn edge_attr<S: Serializer>(&self, edge: &E, s: S) -> Result<S::Ok, S::Error>;
    fn edge_meta<S: Serializer>(&self, edge: &E, s: S) -> Result<S::Ok, S::Error>;
    /// The graph-level `meta` map.
    fn graph_meta<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error>;
}

/// A `Value` in the tagged encoding, floats optionally at half precision.
struct Tagged<'a> {
    value: &'a Value,
    half: bool,
    depth: usize,
}

impl Serialize for Tagged<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let (half, depth) = (self.half, self.depth);
        tagged::check_depth(depth)?;
        match self.value {
            Value::String(v) => tagged::string(s, v),
            Value::Int(v) => tagged::int(s, *v),
            Value::Float(v) => tagged::float(s, *v, half),
            Value::Half(v) => tagged::half(s, *v),
            Value::Bool(v) => tagged::bool(s, *v),
            Value::None => tagged::none(s),
            Value::List(items) => tagged::list(s, &TaggedList { items, half, depth }),
            Value::Dict(map) => tagged::dict(s, &TaggedMap { map, half, depth }),
            Value::Bytes(v) => tagged::bytes(s, v),
            Value::Date(v) => tagged::date(s, *v),
            Value::DateTime(v) => tagged::datetime(s, *v),
        }
    }
}

/// Items of a list value at `depth`.
struct TaggedList<'a> {
    items: &'a [Value],
    half: bool,
    depth: usize,
}

impl Serialize for TaggedList<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(self.items.len()))?;
        for value in self.items {
            seq.serialize_element(&Tagged { value, half: self.half, depth: self.depth + 1 })?;
        }
        seq.end()
    }
}

/// Entries of a dict value at `depth` (0 for an attribute map).
struct TaggedMap<'a> {
    map: &'a Attrs,
    half: bool,
    depth: usize,
}

impl Serialize for TaggedMap<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(self.map.len()))?;
        // Sorted by key, so equal graphs save to the same bytes
        for (k, value) in crate::value::sorted_entries(self.map) {
            map.serialize_entry(k, &Tagged { value, half: self.half, depth: self.depth + 1 })?;
        }
        map.end()
    }
}

/// Codec for `Graph<Record, Record>`.
pub struct RecordCodec<'a> {
    /// Graph-level meta.
    pub meta: &'a Attrs,
    /// Store floats at half precision.
    pub half: bool,
}

impl RecordCodec<'_> {
    fn attrs<S: Serializer>(&self, map: &Attrs, s: S) -> Result<S::Ok, S::Error> {
        TaggedMap { map, half: self.half, depth: 0 }.serialize(s)
    }
}

impl Codec<Record, Record> for RecordCodec<'_> {
    fn node_attr<S: Serializer>(&self, node: &Record, s: S) -> Result<S::Ok, S::Error> {
        self.attrs(&node.attr, s)
    }
    fn node_meta<S: Serializer>(&self, node: &Record, s: S) -> Result<S::Ok, S::Error> {
        self.attrs(&node.meta, s)
    }
    fn edge_attr<S: Serializer>(&self, edge: &Record, s: S) -> Result<S::Ok, S::Error> {
        self.attrs(&edge.attr, s)
    }
    fn edge_meta<S: Serializer>(&self, edge: &Record, s: S) -> Result<S::Ok, S::Error> {
        self.attrs(&edge.meta, s)
    }
    fn graph_meta<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.attrs(self.meta, s)
    }
}

/// Serializable view of a whole graph (format version 2).
///
/// Edges are keyed by their `EdgeId` in decimal and written in node order,
/// then in each node's outgoing-edge order. Each node's `edge_ids` /
/// `inverse_edge_ids` list its outgoing / incoming edges in order. Field
/// order matters for the binary encoding and must match `load.rs`.
///
/// Attribute maps of `Record`s are written sorted by key, so two graphs with
/// equal contents and equal slot order save to the same bytes, except for
/// `metadata.timestamp` (the time of the save); turn that off with
/// [`with_timestamp`](Self::with_timestamp) for byte-identical saves.
pub struct GraphWriter<'a, N, E, C> {
    graph: &'a Graph<N, E>,
    codec: &'a C,
    /// Edge id by edge slot (empty for slots without an edge).
    ids: Vec<String>,
    /// `metadata.timestamp`: the current time, a fixed text, or none.
    timestamp: Timestamp,
}

enum Timestamp {
    Now,
    Fixed(String),
    Omitted,
}

impl<'a, N, E, C: Codec<N, E>> GraphWriter<'a, N, E, C> {
    pub fn new(graph: &'a Graph<N, E>, codec: &'a C) -> Self {
        let mut ids = vec![String::new(); graph.edge_bound()];
        for (e, edge) in graph.edges() {
            ids[e.slot()] = edge.id().0.to_string();
        }
        GraphWriter { graph, codec, ids, timestamp: Timestamp::Now }
    }

    /// Set `metadata.timestamp` to `timestamp`, or leave it out (`None`).
    /// By default it is the time of the save (RFC 3339, UTC), which makes
    /// every save of the same graph differ.
    pub fn with_timestamp(mut self, timestamp: Option<String>) -> Self {
        self.timestamp = match timestamp {
            Some(t) => Timestamp::Fixed(t),
            None => Timestamp::Omitted,
        };
        self
    }

    /// Render the graph as JSON bytes (always valid UTF-8).
    pub fn to_json(&self, pretty: bool) -> Result<Vec<u8>, GraphError> {
        let bytes = if pretty { sonic_rs::to_vec_pretty(self) } else { sonic_rs::to_vec(self) };
        bytes.map_err(|e| GraphError::Format(e.to_string()))
    }

    /// Write the graph in the binary format: header, postcard payload,
    /// trailer with the payload's length and CRC32 (see `format/mod.rs`).
    pub fn write_binary<W: Write>(&self, mut writer: W) -> Result<(), GraphError> {
        let io = |e: std::io::Error| GraphError::Format(e.to_string());
        writer.write_all(&super::binary_header()).map_err(io)?;
        let mut body = Checksummed { inner: writer, crc: crc32fast::Hasher::new(), len: 0 };
        super::take_error();
        postcard::to_io(self, &mut body).map_err(super::postcard_error)?;
        let Checksummed { mut inner, crc, len } = body;
        inner.write_all(&super::binary_trailer(len, crc.finalize())).map_err(io)?;
        Ok(())
    }
}

/// Passes writes through, counting bytes and computing their CRC32.
struct Checksummed<W> {
    inner: W,
    crc: crc32fast::Hasher,
    len: u64,
}

impl<W: Write> Write for Checksummed<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.crc.update(&buf[..n]);
        self.len += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

struct IdList<'a> {
    ids: &'a [String],
    edges: &'a [crate::EdgeIx],
}

impl Serialize for IdList<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(self.edges.len()))?;
        for e in self.edges {
            seq.serialize_element(&self.ids[e.slot()])?;
        }
        seq.end()
    }
}

struct NodeView<'a, N, E, C> {
    writer: &'a GraphWriter<'a, N, E, C>,
    node: &'a Node<N>,
}

struct NodesMap<'a, N, E, C>(&'a GraphWriter<'a, N, E, C>);

struct EdgeView<'a, N, E, C> {
    writer: &'a GraphWriter<'a, N, E, C>,
    id: &'a str,
    edge: &'a Edge<E>,
}

struct EdgesMap<'a, N, E, C>(&'a GraphWriter<'a, N, E, C>);

struct GraphMeta<'a, N, E, C>(&'a GraphWriter<'a, N, E, C>);

struct Metadata<'a> {
    node_count: usize,
    edge_count: usize,
    next_edge_id: u64,
    timestamp: &'a Timestamp,
    /// Paths of the property indexes (definitions only; loaders rebuild them).
    indexes: Vec<&'a [String]>,
}

impl Serialize for Metadata<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let timestamp = match self.timestamp {
            Timestamp::Now => Some(chrono::Utc::now().to_rfc3339()),
            Timestamp::Fixed(t) => Some(t.clone()),
            Timestamp::Omitted => None,
        };
        let mut entries: Vec<(&str, Value)> = vec![
            ("version", Value::String(super::FORMAT_VERSION.to_string())),
            ("node_count", Value::Int(self.node_count as i64)),
            ("edge_count", Value::Int(self.edge_count as i64)),
        ];
        if let Some(t) = timestamp {
            entries.push(("timestamp", Value::String(t)));
        }
        // A string: the full u64 range doesn't fit an Int
        entries.push(("next_edge_id", Value::String(self.next_edge_id.to_string())));
        if !self.indexes.is_empty() {
            let path = |p: &&[String]| Value::List(p.iter().map(|k| Value::String(k.clone())).collect());
            entries.push(("indexes", Value::List(self.indexes.iter().map(path).collect())));
        }
        let mut map = s.serialize_map(Some(entries.len()))?;
        for (k, value) in &entries {
            map.serialize_entry(k, &Tagged { value, half: false, depth: 1 })?;
        }
        map.end()
    }
}

impl<N, E, C: Codec<N, E>> Serialize for NodeView<'_, N, E, C> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let (codec, ids) = (self.writer.codec, &self.writer.ids);
        let data = &self.node.data;
        let graph = self.writer.graph;
        let labels: Vec<&str> = self.node.labels().iter().map(|&l| graph.symbol_name(l)).collect();
        let mut st = s.serialize_struct("SerializableNode", 6)?;
        st.serialize_field("id", self.node.id())?;
        st.serialize_field("labels", &labels)?;
        st.serialize_field("attr", &NodePart::<N, E, C> { codec, data, meta: false, _e: std::marker::PhantomData })?;
        st.serialize_field("meta", &NodePart::<N, E, C> { codec, data, meta: true, _e: std::marker::PhantomData })?;
        st.serialize_field("edge_ids", &IdList { ids, edges: self.node.out_edges() })?;
        st.serialize_field("inverse_edge_ids", &IdList { ids, edges: self.node.in_edges() })?;
        st.end()
    }
}

struct NodePart<'a, N, E, C> {
    codec: &'a C,
    data: &'a N,
    meta: bool,
    _e: std::marker::PhantomData<fn(&E)>,
}

impl<N, E, C: Codec<N, E>> Serialize for NodePart<'_, N, E, C> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if self.meta {
            self.codec.node_meta(self.data, s)
        } else {
            self.codec.node_attr(self.data, s)
        }
    }
}

struct EdgePart<'a, N, E, C> {
    codec: &'a C,
    data: &'a E,
    meta: bool,
    _n: std::marker::PhantomData<fn(&N)>,
}

impl<N, E, C: Codec<N, E>> Serialize for EdgePart<'_, N, E, C> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if self.meta {
            self.codec.edge_meta(self.data, s)
        } else {
            self.codec.edge_attr(self.data, s)
        }
    }
}

impl<N, E, C: Codec<N, E>> Serialize for NodesMap<'_, N, E, C> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let writer = self.0;
        let mut map = s.serialize_map(Some(writer.graph.node_count()))?;
        for (_, node) in writer.graph.nodes() {
            map.serialize_entry(node.id(), &NodeView { writer, node })?;
        }
        map.end()
    }
}

impl<N, E, C: Codec<N, E>> Serialize for EdgeView<'_, N, E, C> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let graph = self.writer.graph;
        let codec = self.writer.codec;
        let data = &self.edge.data;
        let from = graph.node(self.edge.source()).expect("live edge source");
        let to = graph.node(self.edge.target()).expect("live edge target");
        let ty = self.edge.edge_type().map(|t| graph.symbol_name(t));
        let mut st = s.serialize_struct("SerializableEdge", 6)?;
        st.serialize_field("id", self.id)?;
        st.serialize_field("from_id", from.id())?;
        st.serialize_field("to_id", to.id())?;
        st.serialize_field("type", &ty)?;
        st.serialize_field("attr", &EdgePart::<N, E, C> { codec, data, meta: false, _n: std::marker::PhantomData })?;
        st.serialize_field("meta", &EdgePart::<N, E, C> { codec, data, meta: true, _n: std::marker::PhantomData })?;
        st.end()
    }
}

impl<N, E, C: Codec<N, E>> Serialize for EdgesMap<'_, N, E, C> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let writer = self.0;
        let graph = writer.graph;
        let mut map = s.serialize_map(Some(graph.edge_count()))?;
        // Numbering order: nodes in order, each node's outgoing edges in order
        for (_, node) in graph.nodes() {
            for &e in node.out_edges() {
                let id = writer.ids[e.slot()].as_str();
                map.serialize_entry(id, &EdgeView { writer, id, edge: graph.edge_ref(e) })?;
            }
        }
        map.end()
    }
}

impl<N, E, C: Codec<N, E>> Serialize for GraphMeta<'_, N, E, C> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.codec.graph_meta(s)
    }
}

impl<N, E, C: Codec<N, E>> Serialize for GraphWriter<'_, N, E, C> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut st = s.serialize_struct("SerializableGraph", 4)?;
        st.serialize_field("nodes", &NodesMap(self))?;
        st.serialize_field("edges", &EdgesMap(self))?;
        st.serialize_field("meta", &GraphMeta(self))?;
        st.serialize_field(
            "metadata",
            &Metadata {
                node_count: self.graph.node_count(),
                edge_count: self.graph.edge_count(),
                next_edge_id: self.graph.next_edge_id().0,
                timestamp: &self.timestamp,
                indexes: self.graph.index_paths(),
            },
        )?;
        st.end()
    }
}
