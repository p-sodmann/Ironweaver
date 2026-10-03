// format/stream.rs
//
// Loading a binary file from a reader: the payload is decoded as it is
// read and the graph built as it goes, one node or edge entry at a time,
// so the file's bytes are never all in memory next to the graph (the
// slice-based loader needs both at its peak).
//
// The CRC32 of the payload is computed while reading. The trailer comes
// last, so it is checked only at the end: until then everything is held
// back, and a length or checksum mismatch drops the partly built graph
// and returns an error. On any decoding error the rest of the input is
// read too, so a damaged file is reported as damaged (as the slice-based
// loader does) rather than as whatever the damage made the decoder trip
// over.
//
// Files without the header (version 1 binary files among them) are refused
// after reading the header's 16 bytes, with the slice loader's message.

use std::io::{self, Read};

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};

use super::load::{owned_strings, LoadAttrs, LoadEdge, LoadGraph, LoadKind, LoadNode, LoadValue, Str};
use super::{bad_binary, check_trailer, check_version, unframed, HEADER_LEN, MAGIC, TRAILER_LEN};
use crate::{EdgeId, EdgeIx, Graph, GraphError, NodeIx};

/// Read buffer size.
const BUF_LEN: usize = 64 * 1024;

/// The part of a binary file after the header, read from `inner`. Keeps
/// the CRC32 of everything but the last `TRAILER_LEN` bytes read so far,
/// which is the payload's once the input ends.
struct Framed<R> {
    inner: R,
    buf: Vec<u8>,
    /// Next unread byte in `buf`.
    pos: usize,
    /// End of the data in `buf`.
    end: usize,
    /// First byte of `buf` not yet in `crc`.
    unhashed: usize,
    crc: crc32fast::Hasher,
    /// Bytes in `crc`.
    hashed: u64,
    /// Bytes handed to the decoder.
    consumed: u64,
    /// Holds the bytes of `try_take_n_temp`.
    scratch: Vec<u8>,
}

impl<R: Read> Framed<R> {
    fn new(inner: R) -> Self {
        Framed {
            inner,
            buf: vec![0; BUF_LEN + TRAILER_LEN],
            pos: 0,
            end: 0,
            unhashed: 0,
            crc: crc32fast::Hasher::new(),
            hashed: 0,
            consumed: 0,
            scratch: Vec::new(),
        }
    }

    /// Make an unread byte available; false at the end of the input.
    fn fill(&mut self) -> io::Result<bool> {
        if self.pos < self.end {
            return Ok(true);
        }
        // All but the last TRAILER_LEN bytes are payload: hash them, and
        // move the rest to the front
        if self.end - self.unhashed > TRAILER_LEN {
            let upto = self.end - TRAILER_LEN;
            self.crc.update(&self.buf[self.unhashed..upto]);
            self.hashed += (upto - self.unhashed) as u64;
            self.unhashed = upto;
        }
        self.buf.copy_within(self.unhashed..self.end, 0);
        self.end -= self.unhashed;
        self.pos = self.end;
        self.unhashed = 0;
        loop {
            match self.inner.read(&mut self.buf[self.end..]) {
                Ok(0) => return Ok(false),
                Ok(n) => {
                    self.end += n;
                    return Ok(true);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }

    fn byte(&mut self) -> io::Result<Option<u8>> {
        if !self.fill()? {
            return Ok(None);
        }
        let b = self.buf[self.pos];
        self.pos += 1;
        self.consumed += 1;
        Ok(Some(b))
    }

    /// Read `n` bytes into `scratch`; false if the input ends first. The
    /// buffer grows only as data arrives, so a bogus length can't make it
    /// allocate more than the input holds.
    fn take(&mut self, n: usize) -> io::Result<bool> {
        self.scratch.clear();
        while self.scratch.len() < n {
            if !self.fill()? {
                return Ok(false);
            }
            let k = (n - self.scratch.len()).min(self.end - self.pos);
            self.scratch.extend_from_slice(&self.buf[self.pos..self.pos + k]);
            self.pos += k;
            self.consumed += k as u64;
        }
        Ok(true)
    }

    /// Read the rest of the input and check the trailer: the payload is
    /// everything after the header but the last `TRAILER_LEN` bytes.
    fn finish(&mut self) -> Result<(), GraphError> {
        let io = |e: io::Error| GraphError::Format(e.to_string());
        self.pos = self.end;
        while self.fill().map_err(io)? {
            self.pos = self.end;
        }
        // `fill` left at most TRAILER_LEN bytes unhashed
        let trailer = &self.buf[self.unhashed..self.end];
        if trailer.len() < TRAILER_LEN {
            return Err(bad_binary("truncated"));
        }
        check_trailer(trailer, self.hashed, self.crc.clone().finalize())?;
        if self.consumed > self.hashed {
            // The document went on into the trailer
            return Err(bad_binary("truncated payload"));
        }
        Ok(())
    }
}

/// An I/O error while decoding: postcard has no error carrying a message,
/// so the message is remembered (see `super::remember`).
fn io_error(e: io::Error) -> postcard::Error {
    super::remember(&e.to_string());
    postcard::Error::SerdeDeCustom
}

impl<'r, R: Read + 'r> postcard::de_flavors::Flavor<'r> for Framed<R> {
    type Remainder = Self;
    type Source = R;

    fn pop(&mut self) -> postcard::Result<u8> {
        match self.byte() {
            Ok(Some(b)) => Ok(b),
            Ok(None) => Err(postcard::Error::DeserializeUnexpectedEnd),
            Err(e) => Err(io_error(e)),
        }
    }

    fn try_take_n(&mut self, _ct: usize) -> postcard::Result<&'r [u8]> {
        // Nothing can be borrowed from a stream; the loader's types read
        // strings and bytes as owned (`owned_strings`, `deserialize_byte_buf`)
        super::remember("internal error: borrowing from a stream");
        Err(postcard::Error::SerdeDeCustom)
    }

    fn try_take_n_temp<'a>(&'a mut self, ct: usize) -> postcard::Result<&'a [u8]>
    where
        'r: 'a,
    {
        match self.take(ct) {
            Ok(true) => Ok(&self.scratch),
            Ok(false) => Err(postcard::Error::DeserializeUnexpectedEnd),
            Err(e) => Err(io_error(e)),
        }
    }

    fn finalize(self) -> postcard::Result<Self> {
        Ok(self)
    }
}

/// Builds the graph from the entries as they are decoded. The first error
/// (from a callback or the graph) is kept in `error`, and decoding is
/// stopped with a placeholder serde error.
struct Builder<N, E, X, FN, FE> {
    graph: Graph<N, E>,
    make_node: FN,
    make_edge: FE,
    error: Option<X>,
    /// Nodes in document order.
    nodes: Vec<NodeIx>,
    /// Each node's saved outgoing / incoming edge ids, concatenated;
    /// `*_end[k]` is where node `k`'s end.
    out_ids: Vec<u64>,
    out_end: Vec<usize>,
    in_ids: Vec<u64>,
    in_end: Vec<usize>,
    /// Edges in document order.
    edges: Vec<EdgeIx>,
}

impl<N, E, X, FN, FE> Builder<N, E, X, FN, FE>
where
    X: From<GraphError>,
    FN: FnMut(&LoadNode<'_>) -> Result<N, X>,
    FE: FnMut(&LoadEdge<'_>) -> Result<E, X>,
{
    fn fail<T>(&mut self, x: impl Into<X>) -> Result<T, ()> {
        self.error = Some(x.into());
        Err(())
    }

    fn node(&mut self, key: &str, node: &LoadNode<'_>) -> Result<(), ()> {
        let data = match (self.make_node)(node) {
            Ok(d) => d,
            Err(x) => return self.fail(x),
        };
        let ix = match self.graph.add_node(key, data) {
            Ok(ix) => ix,
            Err(e) => return self.fail(e),
        };
        for label in node.labels() {
            if let Err(e) = self.graph.add_label(ix, label) {
                return self.fail(e);
            }
        }
        self.nodes.push(ix);
        // Ids that aren't numbers match no edge (as in `LoadGraph::build`)
        self.out_ids.extend(node.edge_ids.iter().filter_map(|id| id.as_str().parse::<u64>().ok()));
        self.out_end.push(self.out_ids.len());
        self.in_ids.extend(node.inverse_edge_ids.iter().filter_map(|id| id.as_str().parse::<u64>().ok()));
        self.in_end.push(self.in_ids.len());
        Ok(())
    }

    fn edge(&mut self, key: &str, edge: &LoadEdge<'_>) -> Result<(), ()> {
        let lookup = |g: &Graph<N, E>, id: &str, role: &str| {
            g.node_ix(id).ok_or_else(|| GraphError::InvalidArgument(format!("{} node {} not found", role, id)))
        };
        let ends = lookup(&self.graph, edge.from_id(), "From")
            .and_then(|from| Ok((from, lookup(&self.graph, edge.to_id(), "To")?)))
            .and_then(|ends| {
                let id = key.parse().map_err(|_| GraphError::Format(format!("edge id '{}' is not a number", key)))?;
                Ok((ends, EdgeId(id)))
            });
        let ((from, to), id) = match ends {
            Ok(x) => x,
            Err(e) => return self.fail(e),
        };
        let data = match (self.make_edge)(edge) {
            Ok(d) => d,
            Err(x) => return self.fail(x),
        };
        let ty = edge.edge_type().map(|t| self.graph.intern(t));
        match self.graph.add_edge_detached(from, to, Some(id), ty, data) {
            Ok(ix) => self.edges.push(ix),
            Err(e) => return self.fail(e),
        }
        Ok(())
    }

    /// List every edge on its endpoints, in the saved order (see
    /// `LoadGraph::build`), keep removed ids unused and recreate the
    /// saved indexes.
    fn finish(mut self, metadata: &LoadAttrs<'_>) -> Result<Graph<N, E>, GraphError> {
        let g = &mut self.graph;
        if let Some(LoadKind::String(next)) = metadata.get("next_edge_id").map(LoadValue::kind)
            && let Ok(next) = next.parse()
        {
            g.reserve_edge_ids(EdgeId(next));
        }
        let mut out_done = vec![false; g.edge_bound()];
        let mut in_done = vec![false; g.edge_bound()];
        let (mut out_start, mut in_start) = (0, 0);
        for (k, &node) in self.nodes.iter().enumerate() {
            for &id in &self.out_ids[out_start..self.out_end[k]] {
                if let Some(e) = g.edge_ix(EdgeId(id))
                    && !out_done[e.slot()]
                    && g.edge_ref(e).source() == node
                {
                    g.attach_out(e);
                    out_done[e.slot()] = true;
                }
            }
            for &id in &self.in_ids[in_start..self.in_end[k]] {
                if let Some(e) = g.edge_ix(EdgeId(id))
                    && !in_done[e.slot()]
                    && g.edge_ref(e).target() == node
                {
                    g.attach_in(e);
                    in_done[e.slot()] = true;
                }
            }
            (out_start, in_start) = (self.out_end[k], self.in_end[k]);
        }
        for &e in &self.edges {
            if !out_done[e.slot()] {
                g.attach_out(e);
            }
            if !in_done[e.slot()] {
                g.attach_in(e);
            }
        }
        super::load::restore_indexes(&mut self.graph, metadata)?;
        Ok(self.graph)
    }
}

/// The document (positional, as in `LoadGraph`): nodes and edges go to
/// the builder; returns the graph-level `meta` and `metadata`.
struct Document<'b, B>(&'b mut B);

impl<'de, N, E, X, FN, FE> DeserializeSeed<'de> for Document<'_, Builder<N, E, X, FN, FE>>
where
    X: From<GraphError>,
    FN: FnMut(&LoadNode<'_>) -> Result<N, X>,
    FE: FnMut(&LoadEdge<'_>) -> Result<E, X>,
{
    type Value = (LoadAttrs<'de>, LoadAttrs<'de>);

    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_struct("LoadGraph", &["nodes", "edges", "meta", "metadata"], self)
    }
}

impl<'de, N, E, X, FN, FE> Visitor<'de> for Document<'_, Builder<N, E, X, FN, FE>>
where
    X: From<GraphError>,
    FN: FnMut(&LoadNode<'_>) -> Result<N, X>,
    FE: FnMut(&LoadEdge<'_>) -> Result<E, X>,
{
    type Value = (LoadAttrs<'de>, LoadAttrs<'de>);

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a graph document")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let missing = |i| de::Error::invalid_length(i, &"a graph document");
        seq.next_element_seed(Entries { builder: &mut *self.0, nodes: true })?.ok_or_else(|| missing(0))?;
        seq.next_element_seed(Entries { builder: &mut *self.0, nodes: false })?.ok_or_else(|| missing(1))?;
        let meta = seq.next_element()?.ok_or_else(|| missing(2))?;
        let metadata = seq.next_element()?.ok_or_else(|| missing(3))?;
        Ok((meta, metadata))
    }
}

/// The `nodes` or `edges` map, each entry handed to the builder.
struct Entries<'b, B> {
    builder: &'b mut B,
    nodes: bool,
}

impl<'de, N, E, X, FN, FE> DeserializeSeed<'de> for Entries<'_, Builder<N, E, X, FN, FE>>
where
    X: From<GraphError>,
    FN: FnMut(&LoadNode<'_>) -> Result<N, X>,
    FE: FnMut(&LoadEdge<'_>) -> Result<E, X>,
{
    type Value = ();

    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_map(self)
    }
}

impl<'de, N, E, X, FN, FE> Visitor<'de> for Entries<'_, Builder<N, E, X, FN, FE>>
where
    X: From<GraphError>,
    FN: FnMut(&LoadNode<'_>) -> Result<N, X>,
    FE: FnMut(&LoadEdge<'_>) -> Result<E, X>,
{
    type Value = ();

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a map of entries")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        // The builder holds the real error
        let stop = |()| de::Error::custom("stopped by the graph builder");
        while let Some(key) = map.next_key::<Str<'de>>()? {
            if self.nodes {
                let node: LoadNode<'de> = map.next_value()?;
                self.builder.node(key.as_str(), &node).map_err(stop)?;
            } else {
                let edge: LoadEdge<'de> = map.next_value()?;
                self.builder.edge(key.as_str(), &edge).map_err(stop)?;
            }
        }
        Ok(())
    }
}

/// Read up to `buf.len()` bytes; the number read (less only at the end).
fn read_full(r: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

impl LoadGraph<'_> {
    /// Load a binary graph file from `reader`, building the graph while
    /// the payload is decoded, with payloads from `make_node` / `make_edge`
    /// (as [`build`](Self::build) does). Returns the graph and its
    /// graph-level `meta`.
    ///
    /// Peak memory is the graph plus a small buffer, instead of the graph
    /// plus the whole file. The checksum is checked at the end: if it (or
    /// the length) doesn't match, the partly built graph is dropped and
    /// the error returned. Files without the header (version 1 binary
    /// files among them) are a [`GraphError::Format`], as with
    /// [`from_binary_slice`](Self::from_binary_slice). Reads in large
    /// chunks, so `reader` needs no buffering.
    pub fn build_from_reader<R, N, E, X>(
        mut reader: R,
        make_node: impl FnMut(&LoadNode<'_>) -> Result<N, X>,
        make_edge: impl FnMut(&LoadEdge<'_>) -> Result<E, X>,
    ) -> Result<(Graph<N, E>, LoadAttrs<'static>), X>
    where
        R: Read,
        X: From<GraphError>,
    {
        let io = |e: io::Error| GraphError::Format(e.to_string());
        let mut header = [0u8; HEADER_LEN];
        let n = read_full(&mut reader, &mut header).map_err(io)?;
        if !header.starts_with(MAGIC) {
            return Err(unframed(&header[..n]).into());
        }
        if n < HEADER_LEN {
            return Err(bad_binary("truncated").into());
        }
        check_version(&header)?;

        let mut builder = Builder {
            graph: Graph::new(),
            make_node,
            make_edge,
            error: None,
            nodes: Vec::new(),
            out_ids: Vec::new(),
            out_end: Vec::new(),
            in_ids: Vec::new(),
            in_end: Vec::new(),
            edges: Vec::new(),
        };
        super::take_error();
        let mut de = postcard::Deserializer::from_flavor(Framed::new(reader));
        let decoded = owned_strings(|| Document(&mut builder).deserialize(&mut de));
        let mut framed = match de.finalize() {
            Ok(framed) => framed,
            Err(e) => return Err(super::postcard_error(e).into()),
        };
        // A damaged file is reported as such, whatever decoding made of it
        framed.finish()?;
        if let Some(x) = builder.error.take() {
            return Err(x);
        }
        let (meta, metadata) = decoded.map_err(super::postcard_error)?;
        let graph = builder.finish(&metadata)?;
        Ok((graph, meta.into_owned()))
    }
}
