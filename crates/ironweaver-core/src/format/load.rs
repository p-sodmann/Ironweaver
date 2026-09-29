// format/load.rs

use bincode::Options;
use half::f16;
use serde::de::{Deserializer, Error as _, MapAccess, Visitor};
use serde::Deserialize;
use std::borrow::Cow;
use std::cell::Cell;
use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;

use crate::{Attrs, EdgeIx, Graph, GraphError, Value};

/// A string from the input document: borrowed from the input buffer when it
/// contains no escape sequences, owned otherwise.
struct Str<'a>(Cow<'a, str>);

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

        d.deserialize_str(StrVisitor(PhantomData))
    }
}

impl Str<'_> {
    fn as_str(&self) -> &str {
        &self.0
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
            return Err(D::Error::custom(format_args!("attribute values nested more than {MAX_DEPTH} levels deep")));
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
        }
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

    /// An owned copy.
    pub fn to_attrs(&self) -> Attrs {
        self.iter().map(|(k, v)| (k.to_owned(), v.to_value())).collect()
    }
}

/// A node entry of the input document.
#[derive(Deserialize)]
pub struct LoadNode<'a> {
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

impl<'a> LoadNode<'a> {
    /// The `id` field (the node is keyed by its map key, normally the same).
    pub fn id(&self) -> &str {
        self.id.as_str()
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
    #[serde(borrow)]
    attr: LoadAttrs<'a>,
    #[serde(borrow)]
    meta: LoadAttrs<'a>,
}

impl<'a> LoadEdge<'a> {
    pub fn id(&self) -> &str {
        self.id.as_str()
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
    // Informational only, but part of the (non-self-describing) binary
    // format, so it has to be read.
    #[serde(borrow)]
    #[allow(dead_code)]
    metadata: LoadAttrs<'a>,
}

impl<'a> LoadGraph<'a> {
    /// Parse a JSON document.
    pub fn from_json_slice(bytes: &'a [u8]) -> Result<Self, GraphError> {
        sonic_rs::from_slice(bytes).map_err(|e| GraphError::Format(e.to_string()))
    }

    /// Parse a bincode document.
    pub fn from_binary_slice(bytes: &'a [u8]) -> Result<Self, GraphError> {
        let options = bincode::DefaultOptions::new().with_fixint_encoding().allow_trailing_bytes();
        options.deserialize(bytes).map_err(|e| GraphError::Format(e.to_string()))
    }

    /// The graph-level `meta` map.
    pub fn meta(&self) -> &LoadAttrs<'a> {
        &self.meta
    }

    pub fn node_count(&self) -> usize {
        self.nodes.0.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.0.len()
    }

    /// Build the graph, with payloads from `make_node` / `make_edge`.
    ///
    /// Nodes are keyed by their map key and created in document order, then
    /// edges in document order. Each node lists its edges in the order of
    /// its `edge_ids` / `inverse_edge_ids`, so edge order survives a round
    /// trip; edges missing from those lists (hand-written or older files)
    /// are appended.
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
            graph.add_node(key.as_str(), data)?;
        }

        let mut edge_ixs: Vec<EdgeIx> = Vec::with_capacity(self.edges.0.len());
        let mut by_key: HashMap<&str, usize> = HashMap::with_capacity(self.edges.0.len());
        for (key, edge) in &self.edges.0 {
            let lookup = |id: &str, role: &str| {
                graph.node_ix(id).ok_or_else(|| GraphError::InvalidArgument(format!("{} node {} not found", role, id)))
            };
            let from = lookup(edge.from_id(), "From")?;
            let to = lookup(edge.to_id(), "To")?;
            let data = make_edge(edge)?;
            by_key.insert(key.as_str(), edge_ixs.len());
            edge_ixs.push(graph.add_edge_detached(from, to, data)?);
        }

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
