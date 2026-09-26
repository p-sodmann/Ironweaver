// serialization.rs
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
// Every Python value is encoded as an externally tagged `SerializableValue`
// (e.g. `{"Float": 1.5}`); files written by older versions still load.
//
// Saving streams the live graph straight into the serializer through
// `GraphView` (no intermediate copy of the graph). Loading parses into the
// borrowed `Load*` structs (strings point into the input buffer wherever
// possible) and then builds the Python objects.

use pyo3::prelude::*;
use pyo3::types::{
    PyAny, PyBool, PyDict, PyFloat, PyInt, PyList, PyMapping, PySequence, PyString, PyTuple,
};
use serde::de::{Deserializer, MapAccess, Visitor};
use serde::ser::{Error as _, SerializeMap, SerializeSeq, SerializeStruct, Serializer};
use serde::{Deserialize, Serialize};
use half::f16;
use bincode::Options;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::io::Write;
use std::marker::PhantomData;
use crate::{Node, Edge, Vertex};

/// Encoding of a single Python value.
///
/// The variant order is part of the binary format: `PyValue` and `LoadValue`
/// refer to these variants by index, so never reorder them.
#[derive(Serialize, Debug, Clone)]
pub enum SerializableValue {
    String(String),
    Int(i64),
    Float(f64),
    Half(f16),
    Bool(bool),
    None,
    List(Vec<SerializableValue>),
    Dict(HashMap<String, SerializableValue>),
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Dictionary keys are serialized as strings; non-string keys use `str(key)`.
fn dict_key(key: &Bound<'_, PyAny>) -> PyResult<String> {
    match key.downcast::<PyString>() {
        Ok(s) => Ok(s.to_str()?.to_owned()),
        Err(_) => key.str()?.extract(),
    }
}

/// A plain Python value (dict/list/str/number/bool/None) written as ordinary
/// JSON. Used to turn a graph *dict* into JSON bytes, which then go through
/// the same fast loader as JSON text.
struct PlainPy<'a, 'py>(&'a Bound<'py, PyAny>);

impl Serialize for PlainPy<'_, '_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let v = self.0;
        if v.is_none() {
            s.serialize_unit()
        } else if let Ok(b) = v.downcast::<PyBool>() {
            s.serialize_bool(b.is_true())
        } else if let Ok(i) = v.downcast::<PyInt>() {
            match i.extract::<i64>() {
                Ok(n) => s.serialize_i64(n),
                Err(_) => s.serialize_f64(i.extract::<f64>().map_err(py_err)?),
            }
        } else if let Ok(f) = v.downcast::<PyFloat>() {
            s.serialize_f64(f.value())
        } else if let Ok(st) = v.downcast::<PyString>() {
            s.serialize_str(st.to_str().map_err(py_err)?)
        } else if let Ok(dict) = v.downcast::<PyDict>() {
            let mut map = s.serialize_map(Some(dict.len()))?;
            for (k, val) in dict.iter() {
                map.serialize_entry(&dict_key(&k).map_err(py_err)?, &PlainPy(&val))?;
            }
            map.end()
        } else if let Ok(list) = v.downcast::<PyList>() {
            let mut seq = s.serialize_seq(Some(list.len()))?;
            for item in list.iter() {
                seq.serialize_element(&PlainPy(&item))?;
            }
            seq.end()
        } else if let Ok(tuple) = v.downcast::<PyTuple>() {
            let mut seq = s.serialize_seq(Some(tuple.len()))?;
            for item in tuple.iter() {
                seq.serialize_element(&PlainPy(&item))?;
            }
            seq.end()
        } else {
            let name = v.get_type().name().map(|n| n.to_string()).unwrap_or_default();
            Err(S::Error::custom(format!("Unsupported value in graph dict: {}", name)))
        }
    }
}

/// Encode a graph dict (as produced by `json.loads(save_to_json())`) as JSON.
pub fn dict_to_json(obj: &Bound<'_, PyAny>) -> Result<Vec<u8>, BoxError> {
    Ok(sonic_rs::to_vec(&PlainPy(obj))?)
}

// ---------------------------------------------------------------------------
// Saving: stream the live graph
// ---------------------------------------------------------------------------

const VALUE_ENUM: &str = "SerializableValue";
// Variant indices of `SerializableValue`
const V_STRING: u32 = 0;
const V_INT: u32 = 1;
const V_FLOAT: u32 = 2;
const V_HALF: u32 = 3;
const V_BOOL: u32 = 4;
const V_NONE: u32 = 5;
const V_LIST: u32 = 6;
const V_DICT: u32 = 7;

fn py_err<E: serde::ser::Error>(e: PyErr) -> E {
    E::custom(e)
}

/// A Python value serialized as a `SerializableValue`, without first being
/// converted into one. Type dispatch matches the loader's expectations:
/// `bool` is checked before `int` (it is an `int` subclass), numpy arrays and
/// scalars go through one bulk `tolist()`, anything else falls back to `str()`.
struct PyValue<'a, 'py> {
    value: &'a Bound<'py, PyAny>,
    half: bool,
}

impl Serialize for PyValue<'_, '_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let v = self.value;
        let half = self.half;
        let float = |s: S, f: f64| {
            if half {
                s.serialize_newtype_variant(VALUE_ENUM, V_HALF, "Half", &f16::from_f64(f))
            } else {
                s.serialize_newtype_variant(VALUE_ENUM, V_FLOAT, "Float", &f)
            }
        };

        if v.is_none() {
            s.serialize_unit_variant(VALUE_ENUM, V_NONE, "None")
        } else if let Ok(b) = v.downcast::<PyBool>() {
            s.serialize_newtype_variant(VALUE_ENUM, V_BOOL, "Bool", &b.is_true())
        } else if let Ok(i) = v.downcast::<PyInt>() {
            match i.extract::<i64>() {
                Ok(n) => s.serialize_newtype_variant(VALUE_ENUM, V_INT, "Int", &n),
                // Out of i64 range: keep the magnitude as a float
                Err(_) => float(s, i.extract::<f64>().map_err(py_err)?),
            }
        } else if let Ok(f) = v.downcast::<PyFloat>() {
            float(s, f.value())
        } else if let Ok(st) = v.downcast::<PyString>() {
            s.serialize_newtype_variant(VALUE_ENUM, V_STRING, "String", st.to_str().map_err(py_err)?)
        } else if let Ok(list) = v.downcast::<PyList>() {
            let items: Vec<Bound<'_, PyAny>> = list.iter().collect();
            s.serialize_newtype_variant(VALUE_ENUM, V_LIST, "List", &PyItems { items: &items, half })
        } else if let Ok(tuple) = v.downcast::<PyTuple>() {
            let items: Vec<Bound<'_, PyAny>> = tuple.iter().collect();
            s.serialize_newtype_variant(VALUE_ENUM, V_LIST, "List", &PyItems { items: &items, half })
        } else if let Ok(dict) = v.downcast::<PyDict>() {
            let entries: Vec<(Bound<'_, PyAny>, Bound<'_, PyAny>)> = dict.iter().collect();
            s.serialize_newtype_variant(VALUE_ENUM, V_DICT, "Dict", &PyEntries { entries: &entries, half })
        } else if v.hasattr("tolist").map_err(py_err)? {
            // numpy arrays and numpy scalars (e.g. embeddings)
            let native = v.call_method0("tolist").map_err(py_err)?;
            PyValue { value: &native, half }.serialize(s)
        } else if let Ok(mapping) = v.downcast::<PyMapping>() {
            let mut entries = Vec::new();
            for item in mapping.items().map_err(py_err)?.iter() {
                entries.push(item.extract::<(Bound<'_, PyAny>, Bound<'_, PyAny>)>().map_err(py_err)?);
            }
            s.serialize_newtype_variant(VALUE_ENUM, V_DICT, "Dict", &PyEntries { entries: &entries, half })
        } else if let Ok(seq) = v.downcast::<PySequence>() {
            let mut items = Vec::new();
            for item in seq.try_iter().map_err(py_err)? {
                items.push(item.map_err(py_err)?);
            }
            s.serialize_newtype_variant(VALUE_ENUM, V_LIST, "List", &PyItems { items: &items, half })
        } else {
            // Fallback: string representation
            let text = v.str().map_err(py_err)?;
            s.serialize_newtype_variant(VALUE_ENUM, V_STRING, "String", text.to_str().map_err(py_err)?)
        }
    }
}

struct PyItems<'a, 'py> {
    items: &'a [Bound<'py, PyAny>],
    half: bool,
}

impl Serialize for PyItems<'_, '_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(self.items.len()))?;
        for item in self.items {
            seq.serialize_element(&PyValue { value: item, half: self.half })?;
        }
        seq.end()
    }
}

struct PyEntries<'a, 'py> {
    entries: &'a [(Bound<'py, PyAny>, Bound<'py, PyAny>)],
    half: bool,
}

impl Serialize for PyEntries<'_, '_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(self.entries.len()))?;
        for (k, v) in self.entries {
            let key = dict_key(k).map_err(py_err)?;
            map.serialize_entry(&key, &PyValue { value: v, half: self.half })?;
        }
        map.end()
    }
}

/// A node/edge `attr` or `meta` map.
struct AttrMap<'a, 'py> {
    py: Python<'py>,
    map: &'a HashMap<String, Py<PyAny>>,
    half: bool,
}

impl Serialize for AttrMap<'_, '_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(self.map.len()))?;
        for (k, v) in self.map {
            map.serialize_entry(k, &PyValue { value: v.bind(self.py), half: self.half })?;
        }
        map.end()
    }
}

/// One edge as the saver sees it: its handle, its endpoints and its id
/// (`edge_{n}_{from}_to_{to}`), formatted once and reused wherever the edge
/// is referenced (edges map key, `id` field, both endpoints' id lists).
struct EdgeRec<'py> {
    edge: Bound<'py, Edge>,
    from: Bound<'py, Node>,
    to: Bound<'py, Node>,
    id: String,
}

/// Edge indices grouped per node, stored flat (CSR layout): the edges of
/// node `i` are `items[start[i]..start[i + 1]]`.
struct Grouped {
    start: Vec<usize>,
    items: Vec<usize>,
}

impl Grouped {
    /// `owners[e]` is the node index edge `e` belongs to, if any.
    fn new(n_nodes: usize, owners: &[Option<usize>]) -> Self {
        let mut start = vec![0usize; n_nodes + 1];
        for owner in owners.iter().flatten() {
            start[owner + 1] += 1;
        }
        for i in 0..n_nodes {
            start[i + 1] += start[i];
        }
        let mut fill = start.clone();
        let mut items = vec![0usize; start[n_nodes]];
        for (e, owner) in owners.iter().enumerate() {
            if let Some(o) = owner {
                items[fill[*o]] = e;
                fill[*o] += 1;
            }
        }
        Grouped { start, items }
    }

    fn get(&self, node: usize) -> &[usize] {
        &self.items[self.start[node]..self.start[node + 1]]
    }
}

/// Borrowed, serializable view of a whole `Vertex`.
///
/// Edges are numbered in node iteration order, exactly like the previous
/// implementation. Each node's `edge_ids` / `inverse_edge_ids` are the
/// indices of the edges whose source / target is that node.
pub struct GraphView<'a, 'py> {
    py: Python<'py>,
    vertex: &'a Vertex,
    nodes: Vec<(&'a String, Bound<'py, Node>)>,
    edges: Vec<EdgeRec<'py>>,
    out_ids: Grouped,
    in_ids: Grouped,
    half: bool,
}

impl<'a, 'py> GraphView<'a, 'py> {
    pub fn new(py: Python<'py>, vertex: &'a Vertex, half: bool) -> Self {
        let nodes: Vec<(&String, Bound<'py, Node>)> =
            vertex.nodes.iter().map(|(id, n)| (id, n.bind(py).clone())).collect();
        // Node object -> position, to group edges by endpoint without
        // comparing ids (edges normally point at the vertex's own nodes).
        let position: HashMap<usize, usize> =
            nodes.iter().enumerate().map(|(i, (_, n))| (n.as_ptr() as usize, i)).collect();
        let mut edges = Vec::new();
        let mut sources = Vec::new();
        let mut targets = Vec::new();
        for (_, node) in &nodes {
            for edge in &node.borrow().edges {
                let e = edge.bind(py).clone();
                let (from, to) = {
                    let b = e.borrow();
                    (b.from_node.bind(py).clone(), b.to_node.bind(py).clone())
                };
                let n = edges.len();
                let id = format!("edge_{}_{}_to_{}", n, from.borrow().id, to.borrow().id);
                sources.push(position.get(&(from.as_ptr() as usize)).copied());
                targets.push(position.get(&(to.as_ptr() as usize)).copied());
                edges.push(EdgeRec { edge: e, from, to, id });
            }
        }
        let out_ids = Grouped::new(nodes.len(), &sources);
        let in_ids = Grouped::new(nodes.len(), &targets);
        GraphView { py, vertex, nodes, edges, out_ids, in_ids, half }
    }

    /// Render the graph as JSON bytes (always valid UTF-8).
    pub fn to_json(&self, pretty: bool) -> Result<Vec<u8>, BoxError> {
        Ok(if pretty { sonic_rs::to_vec_pretty(self)? } else { sonic_rs::to_vec(self)? })
    }

    /// Write the graph with bincode (fixed-width integer encoding).
    pub fn write_binary<W: Write>(&self, writer: W) -> Result<(), BoxError> {
        let options = bincode::DefaultOptions::new().with_fixint_encoding();
        self.serialize(&mut bincode::Serializer::new(writer, options))?;
        Ok(())
    }
}

struct IdList<'a, 'py> {
    view: &'a GraphView<'a, 'py>,
    ids: &'a [usize],
}

impl Serialize for IdList<'_, '_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(self.ids.len()))?;
        for &n in self.ids {
            seq.serialize_element(&self.view.edges[n].id)?;
        }
        seq.end()
    }
}

struct NodeView<'a, 'py> {
    view: &'a GraphView<'a, 'py>,
    index: usize,
    id: &'a str,
    node: &'a Bound<'py, Node>,
}

impl Serialize for NodeView<'_, '_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let (py, half) = (self.view.py, self.view.half);
        let node = self.node.borrow();
        let out = self.view.out_ids.get(self.index);
        let inv = self.view.in_ids.get(self.index);
        let mut st = s.serialize_struct("SerializableNode", 5)?;
        st.serialize_field("id", self.id)?;
        st.serialize_field("attr", &AttrMap { py, map: &node.attr, half })?;
        st.serialize_field("meta", &AttrMap { py, map: &node.meta, half })?;
        st.serialize_field("edge_ids", &IdList { view: self.view, ids: out })?;
        st.serialize_field("inverse_edge_ids", &IdList { view: self.view, ids: inv })?;
        st.end()
    }
}

struct NodesMap<'a, 'py>(&'a GraphView<'a, 'py>);

impl Serialize for NodesMap<'_, '_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let view = self.0;
        let mut map = s.serialize_map(Some(view.nodes.len()))?;
        for (index, (id, node)) in view.nodes.iter().enumerate() {
            map.serialize_entry(id.as_str(), &NodeView { view, index, id, node })?;
        }
        map.end()
    }
}

struct EdgeView<'a, 'py> {
    view: &'a GraphView<'a, 'py>,
    n: usize,
}

impl Serialize for EdgeView<'_, '_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let (py, half) = (self.view.py, self.view.half);
        let rec = &self.view.edges[self.n];
        let edge = rec.edge.borrow();
        let mut st = s.serialize_struct("SerializableEdge", 5)?;
        st.serialize_field("id", &rec.id)?;
        st.serialize_field("from_id", &rec.from.borrow().id)?;
        st.serialize_field("to_id", &rec.to.borrow().id)?;
        st.serialize_field("attr", &AttrMap { py, map: &edge.attr, half })?;
        st.serialize_field("meta", &AttrMap { py, map: &edge.meta, half })?;
        st.end()
    }
}

struct EdgesMap<'a, 'py>(&'a GraphView<'a, 'py>);

impl Serialize for EdgesMap<'_, '_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let view = self.0;
        let mut map = s.serialize_map(Some(view.edges.len()))?;
        for (n, rec) in view.edges.iter().enumerate() {
            map.serialize_entry(&rec.id, &EdgeView { view, n })?;
        }
        map.end()
    }
}

struct VertexMeta<'a, 'py>(&'a GraphView<'a, 'py>);

impl Serialize for VertexMeta<'_, '_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let dict = self.0.vertex.meta.bind(self.0.py);
        let entries: Vec<(Bound<'_, PyAny>, Bound<'_, PyAny>)> = dict.iter().collect();
        PyEntries { entries: &entries, half: self.0.half }.serialize(s)
    }
}

struct Metadata<'a, 'py>(&'a GraphView<'a, 'py>);

impl Serialize for Metadata<'_, '_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let view = self.0;
        let timestamp = chrono::Utc::now().to_rfc3339();
        let entries: [(&str, SerializableValue); 4] = [
            ("version", SerializableValue::String("1.0".to_string())),
            ("node_count", SerializableValue::Int(view.nodes.len() as i64)),
            ("edge_count", SerializableValue::Int(view.edges.len() as i64)),
            ("timestamp", SerializableValue::String(timestamp)),
        ];
        let mut map = s.serialize_map(Some(entries.len()))?;
        for (k, v) in &entries {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

impl Serialize for GraphView<'_, '_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut st = s.serialize_struct("SerializableGraph", 4)?;
        st.serialize_field("nodes", &NodesMap(self))?;
        st.serialize_field("edges", &EdgesMap(self))?;
        st.serialize_field("meta", &VertexMeta(self))?;
        st.serialize_field("metadata", &Metadata(self))?;
        st.end()
    }
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

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

/// Mirror of `SerializableValue` for loading (same variant order).
#[derive(Deserialize)]
enum LoadValue<'a> {
    String(#[serde(borrow)] Str<'a>),
    Int(i64),
    Float(f64),
    Half(f16),
    Bool(bool),
    None,
    List(#[serde(borrow)] Vec<LoadValue<'a>>),
    Dict(#[serde(borrow)] Entries<Str<'a>, LoadValue<'a>>),
}

type Attrs<'a> = Entries<Str<'a>, LoadValue<'a>>;

#[derive(Deserialize)]
struct LoadNode<'a> {
    #[serde(borrow)]
    id: Str<'a>,
    #[serde(borrow)]
    attr: Attrs<'a>,
    #[serde(borrow)]
    meta: Attrs<'a>,
    #[serde(borrow)]
    edge_ids: Vec<Str<'a>>,
    #[serde(borrow)]
    inverse_edge_ids: Vec<Str<'a>>,
}

#[derive(Deserialize)]
struct LoadEdge<'a> {
    #[serde(borrow)]
    id: Str<'a>,
    #[serde(borrow)]
    from_id: Str<'a>,
    #[serde(borrow)]
    to_id: Str<'a>,
    #[serde(borrow)]
    attr: Attrs<'a>,
    #[serde(borrow)]
    meta: Attrs<'a>,
}

/// A parsed graph document, borrowing from the input buffer.
#[derive(Deserialize)]
pub struct LoadGraph<'a> {
    #[serde(borrow)]
    nodes: Entries<Str<'a>, LoadNode<'a>>,
    #[serde(borrow)]
    edges: Entries<Str<'a>, LoadEdge<'a>>,
    #[serde(borrow)]
    meta: Attrs<'a>,
    // Informational only, but part of the (non-self-describing) binary
    // format, so it has to be read.
    #[serde(borrow)]
    #[allow(dead_code)]
    metadata: Attrs<'a>,
}

/// String values up to this length are shared between all occurrences while
/// loading (e.g. an edge `type` repeated on 100k edges becomes one object).
const SHARED_STRING_MAX: usize = 64;

impl<'a> LoadValue<'a> {
    /// Convert to a Python object. Short strings are looked up in (and added
    /// to) `strings`, so repeated values share one Python object.
    fn to_python<'s>(
        &'s self,
        py: Python<'_>,
        strings: &mut HashMap<&'s str, Py<PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        match self {
            LoadValue::None => Ok(py.None()),
            LoadValue::String(s) => {
                let s = s.as_str();
                if s.len() > SHARED_STRING_MAX {
                    return Ok(PyString::new(py, s).into_any().unbind());
                }
                Ok(strings
                    .entry(s)
                    .or_insert_with(|| PyString::new(py, s).into_any().unbind())
                    .clone_ref(py))
            }
            LoadValue::Int(i) => Ok(i.into_pyobject(py)?.into_any().unbind()),
            LoadValue::Float(f) => Ok(f.into_pyobject(py)?.into_any().unbind()),
            LoadValue::Half(h) => Ok(h.to_f64().into_pyobject(py)?.into_any().unbind()),
            LoadValue::Bool(b) => Ok(b.into_pyobject(py)?.to_owned().into_any().unbind()),
            LoadValue::List(list) => {
                let py_list = PyList::empty(py);
                for item in list {
                    py_list.append(item.to_python(py, strings)?)?;
                }
                Ok(py_list.into_any().unbind())
            }
            LoadValue::Dict(dict) => {
                let py_dict = PyDict::new(py);
                for (key, value) in &dict.0 {
                    py_dict.set_item(key.as_str(), value.to_python(py, strings)?)?;
                }
                Ok(py_dict.into_any().unbind())
            }
        }
    }
}

fn py_map<'s>(
    py: Python<'_>,
    attrs: &'s Attrs<'_>,
    strings: &mut HashMap<&'s str, Py<PyAny>>,
) -> PyResult<HashMap<String, Py<PyAny>>> {
    let mut out = HashMap::with_capacity(attrs.0.len());
    for (key, value) in &attrs.0 {
        out.insert(key.as_str().to_owned(), value.to_python(py, strings)?);
    }
    Ok(out)
}

impl<'a> LoadGraph<'a> {
    /// Parse a JSON document.
    pub fn from_json_slice(bytes: &'a [u8]) -> Result<Self, BoxError> {
        Ok(sonic_rs::from_slice(bytes)?)
    }

    /// Parse a bincode document.
    pub fn from_binary_slice(bytes: &'a [u8]) -> Result<Self, BoxError> {
        let options = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .allow_trailing_bytes();
        Ok(options.deserialize(bytes)?)
    }

    /// Build the Python graph. Nodes and edges are created already pointing
    /// at the new vertex (back-reference) and at its shared update-callback
    /// lists, so the result behaves exactly like an incrementally built graph.
    pub fn into_vertex(&self, py: Python<'_>) -> PyResult<Py<Vertex>> {
        let mut strings: HashMap<&str, Py<PyAny>> = HashMap::new();

        let meta = PyDict::new(py);
        for (key, value) in &self.meta.0 {
            meta.set_item(key.as_str(), value.to_python(py, &mut strings)?)?;
        }
        let mut vertex = Vertex::from_nodes(py, HashMap::new());
        vertex.meta = meta.unbind();
        let node_update = vertex.on_node_update_callbacks.clone_ref(py);
        let edge_update = vertex.on_edge_update_callbacks.clone_ref(py);
        let vertex = Py::new(py, vertex)?;
        let vertex_any: Py<PyAny> = vertex.clone_ref(py).into_any();

        // First pass: create all nodes without edges
        let mut python_nodes: HashMap<String, Py<Node>> = HashMap::with_capacity(self.nodes.0.len());
        for (node_key, node) in &self.nodes.0 {
            let py_node = Py::new(py, Node {
                id: node.id.as_str().to_owned(),
                attr: py_map(py, &node.attr, &mut strings)?,
                meta: py_map(py, &node.meta, &mut strings)?,
                edges: Vec::new(),
                inverse_edges: Vec::new(),
                on_edge_add_callbacks: Vec::new(),
                on_update_callbacks: node_update.clone_ref(py),
                vertex: Some(vertex_any.clone_ref(py)),
            })?;
            python_nodes.insert(node_key.as_str().to_owned(), py_node);
        }

        // Second pass: create all edges (same order as in the document)
        let mut python_edges: Vec<Py<Edge>> = Vec::with_capacity(self.edges.0.len());
        let mut edge_index: HashMap<&str, usize> = HashMap::with_capacity(self.edges.0.len());
        for (edge_key, edge) in &self.edges.0 {
            let lookup = |id: &Str<'_>, role: &str| {
                python_nodes.get(id.as_str()).ok_or_else(|| {
                    PyErr::new::<pyo3::exceptions::PyValueError, _>(format!(
                        "{} node {} not found", role, id.as_str()
                    ))
                })
            };
            let from_node = lookup(&edge.from_id, "From")?;
            let to_node = lookup(&edge.to_id, "To")?;

            let py_edge = Py::new(py, Edge {
                id: Some(edge.id.as_str().to_owned()),
                from_node: from_node.clone_ref(py),
                to_node: to_node.clone_ref(py),
                attr: py_map(py, &edge.attr, &mut strings)?,
                meta: py_map(py, &edge.meta, &mut strings)?,
                watched_by: Vec::new(),
                on_meta_change_callbacks: Vec::new(),
                on_update_callbacks: edge_update.clone_ref(py),
                vertex: Some(vertex_any.clone_ref(py)),
            })?;

            edge_index.insert(edge_key.as_str(), python_edges.len());
            python_edges.push(py_edge);
        }

        // Third pass: attach edges to nodes in their saved order (edge_ids /
        // inverse_edge_ids), so edge order survives a save/load round trip.
        let mut attached = vec![false; python_edges.len()];
        for (node_key, node) in &self.nodes.0 {
            let node_key = node_key.as_str();
            let mut node_ref = python_nodes[node_key].bind(py).borrow_mut();
            for edge_id in &node.edge_ids {
                if let Some(&i) = edge_index.get(edge_id.as_str()) {
                    if self.edges.0[i].1.from_id.as_str() == node_key {
                        node_ref.edges.push(python_edges[i].clone_ref(py));
                        attached[i] = true;
                    }
                }
            }
            for edge_id in &node.inverse_edge_ids {
                if let Some(&i) = edge_index.get(edge_id.as_str()) {
                    if self.edges.0[i].1.to_id.as_str() == node_key {
                        node_ref.inverse_edges.push(python_edges[i].clone_ref(py));
                    }
                }
            }
        }

        // Edges not listed in their source node's edge_ids (hand-written or
        // older files) are still attached.
        for (i, edge) in python_edges.iter().enumerate() {
            if !attached[i] {
                let e = &self.edges.0[i].1;
                python_nodes[e.from_id.as_str()].borrow_mut(py).edges.push(edge.clone_ref(py));
                let to = &python_nodes[e.to_id.as_str()];
                let already = to.borrow(py).inverse_edges.iter().any(|x| x.is(edge));
                if !already {
                    to.borrow_mut(py).inverse_edges.push(edge.clone_ref(py));
                }
            }
        }

        vertex.borrow_mut(py).nodes = python_nodes;
        Ok(vertex)
    }
}
