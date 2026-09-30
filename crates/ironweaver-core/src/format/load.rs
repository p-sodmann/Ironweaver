// format/load.rs

use bincode::Options;
use half::f16;
use serde::de::{Deserializer, MapAccess, Visitor};
use serde::Deserialize;
use std::borrow::Cow;
use std::cell::Cell;
use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;

use crate::{Attrs, Date, DateTime, EdgeId, EdgeIx, Graph, GraphError, Value};

/// A string from the input document: borrowed from the input buffer when it
/// contains no escape sequences, owned otherwise.
pub(super) struct Str<'a>(Cow<'a, str>);

thread_local! {
    // Strings must be read as owned (`deserialize_string`): set while
    // decoding from a stream, where nothing can be borrowed
    static OWNED: Cell<bool> = const { Cell::new(false) };
}

/// Run `f` with strings read as owned copies (see `Str`).
pub(super) fn owned_strings<T>(f: impl FnOnce() -> T) -> T {
    struct Reset(bool);
    impl Drop for Reset {
        fn drop(&mut self) {
            OWNED.with(|c| c.set(self.0));
        }
    }
    let _reset = Reset(OWNED.with(|c| c.replace(true)));
    f()
}

impl<'de: 'a, 'a> Deserialize<'de> for Str<'a> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct StrVisitor<'a>(PhantomData<&'a ()>);

        impl<'de: 'a, 'a> Visitor<'de> for StrVisitor<'a> {
            type Value = Str<'a>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string")
            }
            fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Str<'a>, E> {
                Ok(Str(Cow::Borrowed(v)))
            }
            fn visit_str<E>(self, v: &str) -> Result<Str<'a>, E> {
                Ok(Str(Cow::Owned(v.to_owned())))
            }
            fn visit_string<E>(self, v: String) -> Result<Str<'a>, E> {
                Ok(Str(Cow::Owned(v)))
            }
        }

        if OWNED.with(Cell::get) {
            d.deserialize_string(StrVisitor(PhantomData))
        } else {
            d.deserialize_str(StrVisitor(PhantomData))
        }
    }
}

impl Str<'_> {
    pub(super) fn as_str(&self) -> &str {
        &self.0
    }

    fn into_owned(self) -> Str<'static> {
        Str(Cow::Owned(self.0.into_owned()))
    }
}

/// A map read as a list of entries: no hashing and a single allocation.
struct Entries<K, V>(Vec<(K, V)>);

impl<'de, K: Deserialize<'de>, V: Deserialize<'de>> Deserialize<'de> for Entries<K, V> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct EntriesVisitor<K, V>(PhantomData<(K, V)>);

        impl<'de, K: Deserialize<'de>, V: Deserialize<'de>> Visitor<'de> for EntriesVisitor<K, V> {
            type Value = Entries<K, V>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a map")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut entries = Vec::with_capacity(map.size_hint().unwrap_or(0).min(1 << 16));
                while let Some(entry) = map.next_entry()? {
                    entries.push(entry);
                }
                Ok(Entries(entries))
            }
        }

        d.deserialize_map(EntriesVisitor(PhantomData))
    }
}

/// A value from the input document (borrowing its strings), in the tagged
/// `Value` encoding (same variant order).
#[derive(Deserialize)]
enum RawValue<'a> {
    String(#[serde(borrow)] Str<'a>),
    Int(i64),
    Float(f64),
    Half(f16),
    Bool(bool),
    None,
    List(#[serde(borrow)] Vec<LoadValue<'a>>),
    Dict(#[serde(borrow)] LoadAttrs<'a>),
    Bytes(#[serde(with = "crate::temporal::bytes")] Vec<u8>),
    Date(Date),
    DateTime(DateTime),
}

/// A value from the input document; strings borrow from the input buffer.
pub struct LoadValue<'a>(RawValue<'a>);

/// Deepest nesting of list / dict attribute values a document may contain
/// (a scalar is depth 1). Deeper input is rejected instead of overflowing the
/// stack; savers enforce the same limit, so every saved graph loads again.
pub const MAX_DEPTH: usize = 100;

thread_local! {
    // Nesting depth of the `LoadValue` being parsed on this thread
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

impl<'de: 'a, 'a> Deserialize<'de> for LoadValue<'a> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Level;
        impl Drop for Level {
            fn drop(&mut self) {
                DEPTH.with(|c| c.set(c.get() - 1));
            }
        }

        let depth = DEPTH.with(|c| {
            c.set(c.get() + 1);
            c.get()
        });
        let _level = Level;
        if depth > MAX_DEPTH {
            return Err(super::de_error(format_args!("attribute values nested more than {MAX_DEPTH} levels deep")));
        }
        RawValue::deserialize(d).map(LoadValue)
    }
}

/// What a [`LoadValue`] holds.
pub enum LoadKind<'v, 'a> {
    String(&'v str),
    Int(i64),
    Float(f64),
    Bool(bool),
    None,
    List(&'v [LoadValue<'a>]),
    Dict(&'v LoadAttrs<'a>),
    Bytes(&'v [u8]),
    Date(Date),
    DateTime(DateTime),
}

impl<'a> LoadValue<'a> {
    /// The value's contents (`Half` floats are widened to `Float`).
    pub fn kind(&self) -> LoadKind<'_, 'a> {
        match &self.0 {
            RawValue::String(s) => LoadKind::String(s.as_str()),
            RawValue::Int(i) => LoadKind::Int(*i),
            RawValue::Float(f) => LoadKind::Float(*f),
            RawValue::Half(h) => LoadKind::Float(h.to_f64()),
            RawValue::Bool(b) => LoadKind::Bool(*b),
            RawValue::None => LoadKind::None,
            RawValue::List(items) => LoadKind::List(items),
            RawValue::Dict(d) => LoadKind::Dict(d),
            RawValue::Bytes(b) => LoadKind::Bytes(b),
            RawValue::Date(d) => LoadKind::Date(*d),
            RawValue::DateTime(t) => LoadKind::DateTime(*t),
        }
    }

    /// The same value, owning its strings.
    pub fn into_owned(self) -> LoadValue<'static> {
        LoadValue(match self.0 {
            RawValue::String(s) => RawValue::String(s.into_owned()),
            RawValue::Int(i) => RawValue::Int(i),
            RawValue::Float(f) => RawValue::Float(f),
            RawValue::Half(h) => RawValue::Half(h),
            RawValue::Bool(b) => RawValue::Bool(b),
            RawValue::None => RawValue::None,
            RawValue::List(items) => RawValue::List(items.into_iter().map(LoadValue::into_owned).collect()),
            RawValue::Dict(d) => RawValue::Dict(d.into_owned()),
            RawValue::Bytes(b) => RawValue::Bytes(b),
            RawValue::Date(d) => RawValue::Date(d),
            RawValue::DateTime(t) => RawValue::DateTime(t),
        })
    }

    /// An owned copy.
    pub fn to_value(&self) -> Value {
        match self.kind() {
            LoadKind::String(s) => Value::String(s.to_owned()),
            LoadKind::Int(i) => Value::Int(i),
            LoadKind::Float(f) => match &self.0 {
                RawValue::Half(h) => Value::Half(*h),
                _ => Value::Float(f),
            },
            LoadKind::Bool(b) => Value::Bool(b),
            LoadKind::None => Value::None,
            LoadKind::List(items) => Value::List(items.iter().map(LoadValue::to_value).collect()),
            LoadKind::Dict(d) => Value::Dict(d.to_attrs()),
            LoadKind::Bytes(b) => Value::Bytes(b.to_vec()),
            LoadKind::Date(d) => Value::Date(d),
            LoadKind::DateTime(t) => Value::DateTime(t),
        }
    }
}

/// An attribute map from the input document, in document order.
#[derive(Deserialize)]
#[serde(transparent)]
pub struct LoadAttrs<'a>(#[serde(borrow)] Entries<Str<'a>, LoadValue<'a>>);

impl<'a> LoadAttrs<'a> {
    pub fn len(&self) -> usize {
        self.0 .0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0 .0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &LoadValue<'a>)> + '_ {
        self.0 .0.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// The same map, owning its strings.
    pub fn into_owned(self) -> LoadAttrs<'static> {
        LoadAttrs(Entries(self.0 .0.into_iter().map(|(k, v)| (k.into_owned(), v.into_owned())).collect()))
    }

    /// An owned copy.
    pub fn to_attrs(&self) -> Attrs {
        self.iter().map(|(k, v)| (k.to_owned(), v.to_value())).collect()
    }

    /// The value under `key` (the first, if repeated).
    pub fn get(&self, key: &str) -> Option<&LoadValue<'a>> {
        self.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
    }

    /// Remove and return the entry under `key` if `keep` accepts its value.
    fn take_if(&mut self, key: &str, keep: impl Fn(&LoadValue<'a>) -> bool) -> Option<LoadValue<'a>> {
        let at = self.0 .0.iter().position(|(k, v)| k.as_str() == key && keep(v))?;
        Some(self.0 .0.remove(at).1)
    }
}

impl Default for LoadAttrs<'_> {
    fn default() -> Self {
        LoadAttrs(Entries(Vec::new()))
    }
}

/// A node entry of the input document.
#[derive(Deserialize)]
pub struct LoadNode<'a> {
    #[serde(borrow)]
    id: Str<'a>,
    #[serde(borrow, default)]
    labels: Vec<Str<'a>>,
    #[serde(borrow)]
    attr: LoadAttrs<'a>,
    #[serde(borrow)]
    meta: LoadAttrs<'a>,
    #[serde(borrow)]
    pub(super) edge_ids: Vec<Str<'a>>,
    #[serde(borrow)]
    pub(super) inverse_edge_ids: Vec<Str<'a>>,
}

impl<'a> LoadNode<'a> {
    /// The `id` field (the node is keyed by its map key, normally the same).
    pub fn id(&self) -> &str {
        self.id.as_str()
    }

    pub fn labels(&self) -> impl Iterator<Item = &str> + '_ {
        self.labels.iter().map(Str::as_str)
    }

    pub fn attr(&self) -> &LoadAttrs<'a> {
        &self.attr
    }

    pub fn meta(&self) -> &LoadAttrs<'a> {
        &self.meta
    }
}

/// An edge entry of the input document.
#[derive(Deserialize)]
pub struct LoadEdge<'a> {
    #[serde(borrow)]
    id: Str<'a>,
    #[serde(borrow)]
    from_id: Str<'a>,
    #[serde(borrow)]
    to_id: Str<'a>,
    #[serde(borrow, default, rename = "type")]
    ty: Option<Str<'a>>,
    #[serde(borrow)]
    attr: LoadAttrs<'a>,
    #[serde(borrow)]
    meta: LoadAttrs<'a>,
}

impl<'a> LoadEdge<'a> {
    /// The id in the document: the decimal `EdgeId` (version 2), or the old
    /// string id (version 1, e.g. `edge_0_a_to_b`).
    pub fn id(&self) -> &str {
        self.id.as_str()
    }

    pub fn edge_type(&self) -> Option<&str> {
        self.ty.as_ref().map(Str::as_str)
    }

    pub fn from_id(&self) -> &str {
        self.from_id.as_str()
    }

    pub fn to_id(&self) -> &str {
        self.to_id.as_str()
    }

    pub fn attr(&self) -> &LoadAttrs<'a> {
        &self.attr
    }

    pub fn meta(&self) -> &LoadAttrs<'a> {
        &self.meta
    }
}

/// A parsed graph document, borrowing from the input buffer.
#[derive(Deserialize)]
pub struct LoadGraph<'a> {
    #[serde(borrow)]
    nodes: Entries<Str<'a>, LoadNode<'a>>,
    #[serde(borrow)]
    edges: Entries<Str<'a>, LoadEdge<'a>>,
    #[serde(borrow)]
    meta: LoadAttrs<'a>,
    #[serde(borrow, default)]
    metadata: LoadAttrs<'a>,
    /// Format version (1 or 2), from `metadata` or the binary header.
    #[serde(skip)]
    version: u32,
}

// Version 1 binary documents (bincode, positional): no labels / types.
#[derive(Deserialize)]
struct V1Node<'a> {
    #[serde(borrow)]
    id: Str<'a>,
    #[serde(borrow)]
    attr: LoadAttrs<'a>,
    #[serde(borrow)]
    meta: LoadAttrs<'a>,
    #[serde(borrow)]
    edge_ids: Vec<Str<'a>>,
    #[serde(borrow)]
    inverse_edge_ids: Vec<Str<'a>>,
}

#[derive(Deserialize)]
struct V1Edge<'a> {
    #[serde(borrow)]
    id: Str<'a>,
    #[serde(borrow)]
    from_id: Str<'a>,
    #[serde(borrow)]
    to_id: Str<'a>,
    #[serde(borrow)]
    attr: LoadAttrs<'a>,
    #[serde(borrow)]
    meta: LoadAttrs<'a>,
}

#[derive(Deserialize)]
struct V1Graph<'a> {
    #[serde(borrow)]
    nodes: Entries<Str<'a>, V1Node<'a>>,
    #[serde(borrow)]
    edges: Entries<Str<'a>, V1Edge<'a>>,
    #[serde(borrow)]
    meta: LoadAttrs<'a>,
    #[serde(borrow)]
    metadata: LoadAttrs<'a>,
}

impl<'a> From<V1Graph<'a>> for LoadGraph<'a> {
    fn from(g: V1Graph<'a>) -> Self {
        let nodes = g
            .nodes
            .0
            .into_iter()
            .map(|(k, n)| {
                let V1Node { id, attr, meta, edge_ids, inverse_edge_ids } = n;
                (k, LoadNode { id, labels: Vec::new(), attr, meta, edge_ids, inverse_edge_ids })
            })
            .collect();
        let edges = g
            .edges
            .0
            .into_iter()
            .map(|(k, e)| {
                let V1Edge { id, from_id, to_id, attr, meta } = e;
                (k, LoadEdge { id, from_id, to_id, ty: None, attr, meta })
            })
            .collect();
        LoadGraph { nodes: Entries(nodes), edges: Entries(edges), meta: g.meta, metadata: g.metadata, version: 1 }
    }
}

impl<'a> LoadGraph<'a> {
    /// Parse a JSON document (any version).
    pub fn from_json_slice(bytes: &'a [u8]) -> Result<Self, GraphError> {
        let mut doc: LoadGraph<'a> = sonic_rs::from_slice(bytes).map_err(|e| GraphError::Format(e.to_string()))?;
        doc.version = match doc.metadata.get("version").map(LoadValue::kind) {
            None => 1,
            Some(LoadKind::String(v)) if v.starts_with("1.") || v == "1" => 1,
            Some(LoadKind::String(v)) if v.starts_with("2.") || v == "2" => 2,
            Some(LoadKind::String(v)) => {
                return Err(GraphError::Format(format!(
                    "graph format version {} is not supported (written by a newer ironweaver?)",
                    v
                )))
            }
            Some(_) => return Err(GraphError::Format("metadata.version must be a string".into())),
        };
        if doc.version == 1 {
            doc.migrate_v1();
        }
        Ok(doc)
    }

    /// Parse a binary document: a framed postcard file (version 2) or a
    /// version 1 bincode file.
    pub fn from_binary_slice(bytes: &'a [u8]) -> Result<Self, GraphError> {
        let format = |e: &dyn fmt::Display| GraphError::Format(e.to_string());
        match super::binary_payload(bytes)? {
            Some(payload) => {
                super::take_error();
                let mut doc: LoadGraph<'a> = postcard::from_bytes(payload).map_err(super::postcard_error)?;
                doc.version = 2;
                Ok(doc)
            }
            None => {
                // No size limit needed: every length is checked against the
                // input slice before anything is allocated.
                let options = bincode::DefaultOptions::new().with_fixint_encoding().allow_trailing_bytes();
                let v1: V1Graph<'a> = options.deserialize(bytes).map_err(|e| format(&e))?;
                let mut doc = LoadGraph::from(v1);
                doc.migrate_v1();
                Ok(doc)
            }
        }
    }

    /// Version 1 conventions -> fields: node attribute `labels` (a list of
    /// strings) becomes the labels, edge attribute `type` (a string) the type.
    fn migrate_v1(&mut self) {
        for (_, node) in &mut self.nodes.0 {
            let all_strings = |v: &LoadValue<'_>| match &v.0 {
                RawValue::List(items) => items.iter().all(|i| matches!(i.0, RawValue::String(_))),
                _ => false,
            };
            if let Some(LoadValue(RawValue::List(items))) = node.attr.take_if("labels", all_strings) {
                for item in items {
                    if let RawValue::String(s) = item.0 {
                        if !node.labels.iter().any(|l| l.as_str() == s.as_str()) {
                            node.labels.push(s);
                        }
                    }
                }
            }
        }
        for (_, edge) in &mut self.edges.0 {
            let is_string = |v: &LoadValue<'_>| matches!(v.0, RawValue::String(_));
            if let Some(LoadValue(RawValue::String(s))) = edge.attr.take_if("type", is_string) {
                edge.ty = Some(s);
            }
        }
    }

    /// The document's format version (1 or 2).
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Paths of the property indexes saved with the graph (`metadata.indexes`;
    /// none for files without it).
    pub fn index_paths(&self) -> Result<Vec<Vec<String>>, GraphError> {
        index_paths(&self.metadata)
    }

    /// The graph-level `meta` map.
    pub fn meta(&self) -> &LoadAttrs<'a> {
        &self.meta
    }

    pub(super) fn into_meta(self) -> LoadAttrs<'a> {
        self.meta
    }

    pub fn node_count(&self) -> usize {
        self.nodes.0.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.0.len()
    }

    /// Build the graph, with payloads from `make_node` / `make_edge`.
    ///
    /// Nodes are keyed by their map key and created in document order (with
    /// their labels), then edges in document order (with their types; with
    /// their saved ids for version 2, new ids for version 1). Each node
    /// lists its edges in the order of its `edge_ids` / `inverse_edge_ids`,
    /// so edge order survives a round trip; edges missing from those lists
    /// (hand-written or older files) are appended.
    ///
    /// Property indexes saved with the graph are recreated, empty and with
    /// every node marked dirty (lookups are exact meanwhile, but read the
    /// nodes): call [`Graph::flush_indexes`] to fill them in (the `Record`
    /// loaders in [`format`](crate::format) do).
    pub fn build<'s, N, E, X>(
        &'s self,
        mut make_node: impl FnMut(&'s LoadNode<'a>) -> Result<N, X>,
        mut make_edge: impl FnMut(&'s LoadEdge<'a>) -> Result<E, X>,
    ) -> Result<Graph<N, E>, X>
    where
        X: From<GraphError>,
    {
        let mut graph = Graph::with_capacity(self.nodes.0.len(), self.edges.0.len());
        for (key, node) in &self.nodes.0 {
            let data = make_node(node)?;
            let ix = graph.add_node(key.as_str(), data)?;
            for label in node.labels() {
                graph.add_label(ix, label)?;
            }
        }

        let mut edge_ixs: Vec<EdgeIx> = Vec::with_capacity(self.edges.0.len());
        let mut by_key: HashMap<&str, usize> = HashMap::with_capacity(self.edges.0.len());
        for (key, edge) in &self.edges.0 {
            let lookup = |id: &str, role: &str| {
                graph.node_ix(id).ok_or_else(|| GraphError::InvalidArgument(format!("{} node {} not found", role, id)))
            };
            let from = lookup(edge.from_id(), "From")?;
            let to = lookup(edge.to_id(), "To")?;
            let id = match self.version {
                1 => None,
                _ => Some(EdgeId(
                    key.as_str()
                        .parse()
                        .map_err(|_| GraphError::Format(format!("edge id '{}' is not a number", key.as_str())))?,
                )),
            };
            let data = make_edge(edge)?;
            by_key.insert(key.as_str(), edge_ixs.len());
            let ty = edge.edge_type().map(|t| graph.intern(t));
            edge_ixs.push(graph.add_edge_detached(from, to, id, ty, data)?);
        }
        if let Some(LoadKind::String(next)) = self.metadata.get("next_edge_id").map(LoadValue::kind) {
            if let Ok(next) = next.parse() {
                graph.reserve_edge_ids(EdgeId(next));
            }
        }
        restore_indexes(&mut graph, &self.metadata)?;

        let mut out_done = vec![false; edge_ixs.len()];
        let mut in_done = vec![false; edge_ixs.len()];
        for (key, node) in &self.nodes.0 {
            let key = key.as_str();
            for id in &node.edge_ids {
                if let Some(&i) = by_key.get(id.as_str()) {
                    if !out_done[i] && self.edges.0[i].1.from_id() == key {
                        graph.attach_out(edge_ixs[i]);
                        out_done[i] = true;
                    }
                }
            }
            for id in &node.inverse_edge_ids {
                if let Some(&i) = by_key.get(id.as_str()) {
                    if !in_done[i] && self.edges.0[i].1.to_id() == key {
                        graph.attach_in(edge_ixs[i]);
                        in_done[i] = true;
                    }
                }
            }
        }
        for (i, &e) in edge_ixs.iter().enumerate() {
            if !out_done[i] {
                graph.attach_out(e);
            }
            if !in_done[i] {
                graph.attach_in(e);
            }
        }
        Ok(graph)
    }
}

/// The index paths in a document's `metadata.indexes`: a list of paths,
/// each a list of strings.
pub(super) fn index_paths(metadata: &LoadAttrs<'_>) -> Result<Vec<Vec<String>>, GraphError> {
    let bad = || GraphError::Format("metadata.indexes must be a list of lists of strings".into());
    let Some(value) = metadata.get("indexes") else { return Ok(Vec::new()) };
    let LoadKind::List(paths) = value.kind() else { return Err(bad()) };
    paths
        .iter()
        .map(|p| match p.kind() {
            LoadKind::List(keys) => keys
                .iter()
                .map(|k| match k.kind() {
                    LoadKind::String(s) => Ok(s.to_owned()),
                    _ => Err(bad()),
                })
                .collect(),
            _ => Err(bad()),
        })
        .collect()
}

/// Recreate the saved property indexes, empty: every node is marked dirty,
/// so lookups are exact at once (they read the nodes) and
/// `Graph::flush_indexes` fills them in.
pub(super) fn restore_indexes<N, E>(graph: &mut Graph<N, E>, metadata: &LoadAttrs<'_>) -> Result<(), GraphError> {
    for path in index_paths(metadata)? {
        graph.create_index_with_keys(&path, std::iter::empty())?;
    }
    Ok(())
}
