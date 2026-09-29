// convert.rs
//
// Python values <-> the core's graph document format.
//
// Saving: `PyCodec` encodes node/edge/graph attribute values (arbitrary
// Python objects) straight into the serializer as tagged values, without an
// intermediate `Value`. Loading: `to_python` turns parsed values into Python
// objects, sharing short strings. `dict_to_json` lets a graph *dict* go
// through the JSON loader.

use ironweaver_core::format::{tagged, Codec, LoadAttrs, LoadKind, LoadValue};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyBool, PyDict, PyFloat, PyInt, PyList, PyMapping, PySequence, PyString, PyTuple};
use serde::ser::{Error as _, SerializeMap, SerializeSeq, Serializer};
use serde::Serialize;
use std::collections::HashMap;

use crate::data::{EdgeData, NodeData, PyAttrs};

/// Dictionary keys are serialized as strings; non-string keys use `str(key)`.
fn dict_key(key: &Bound<'_, PyAny>) -> PyResult<String> {
    match key.downcast::<PyString>() {
        Ok(s) => Ok(s.to_str()?.to_owned()),
        Err(_) => key.str()?.extract(),
    }
}

fn py_err<E: serde::ser::Error>(e: PyErr) -> E {
    E::custom(e)
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
pub fn dict_to_json(obj: &Bound<'_, PyAny>) -> Result<Vec<u8>, String> {
    sonic_rs::to_vec(&PlainPy(obj)).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Saving
// ---------------------------------------------------------------------------

/// A Python value serialized as a tagged value. Type dispatch matches the
/// loader's expectations: `bool` is checked before `int` (it is an `int`
/// subclass), numpy arrays and scalars go through one bulk `tolist()`,
/// anything else falls back to `str()`.
struct PyValue<'a, 'py> {
    value: &'a Bound<'py, PyAny>,
    half: bool,
}

impl Serialize for PyValue<'_, '_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let v = self.value;
        let half = self.half;

        if v.is_none() {
            tagged::none(s)
        } else if let Ok(b) = v.downcast::<PyBool>() {
            tagged::bool(s, b.is_true())
        } else if let Ok(i) = v.downcast::<PyInt>() {
            match i.extract::<i64>() {
                Ok(n) => tagged::int(s, n),
                // Out of i64 range: keep the magnitude as a float
                Err(_) => tagged::float(s, i.extract::<f64>().map_err(py_err)?, half),
            }
        } else if let Ok(f) = v.downcast::<PyFloat>() {
            tagged::float(s, f.value(), half)
        } else if let Ok(st) = v.downcast::<PyString>() {
            tagged::string(s, st.to_str().map_err(py_err)?)
        } else if let Ok(list) = v.downcast::<PyList>() {
            let items: Vec<Bound<'_, PyAny>> = list.iter().collect();
            tagged::list(s, &PyItems { items: &items, half })
        } else if let Ok(tuple) = v.downcast::<PyTuple>() {
            let items: Vec<Bound<'_, PyAny>> = tuple.iter().collect();
            tagged::list(s, &PyItems { items: &items, half })
        } else if let Ok(dict) = v.downcast::<PyDict>() {
            let entries: Vec<(Bound<'_, PyAny>, Bound<'_, PyAny>)> = dict.iter().collect();
            tagged::dict(s, &PyEntries { entries: &entries, half })
        } else if v.hasattr("tolist").map_err(py_err)? {
            // numpy arrays and numpy scalars (e.g. embeddings)
            let native = v.call_method0("tolist").map_err(py_err)?;
            PyValue { value: &native, half }.serialize(s)
        } else if let Ok(mapping) = v.downcast::<PyMapping>() {
            let mut entries = Vec::new();
            for item in mapping.items().map_err(py_err)?.iter() {
                entries.push(item.extract::<(Bound<'_, PyAny>, Bound<'_, PyAny>)>().map_err(py_err)?);
            }
            tagged::dict(s, &PyEntries { entries: &entries, half })
        } else if let Ok(seq) = v.downcast::<PySequence>() {
            let mut items = Vec::new();
            for item in seq.try_iter().map_err(py_err)? {
                items.push(item.map_err(py_err)?);
            }
            tagged::list(s, &PyItems { items: &items, half })
        } else {
            // Fallback: string representation
            let text = v.str().map_err(py_err)?;
            tagged::string(s, text.to_str().map_err(py_err)?)
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
            let value = PyValue { value: v, half: self.half };
            match k.downcast::<PyString>() {
                Ok(key) => map.serialize_entry(key.to_str().map_err(py_err)?, &value)?,
                Err(_) => map.serialize_entry(&dict_key(k).map_err(py_err)?, &value)?,
            }
        }
        map.end()
    }
}

/// Encodes the payloads of a Python graph.
pub struct PyCodec<'py> {
    pub py: Python<'py>,
    /// The vertex's `meta` dict.
    pub meta: Bound<'py, PyDict>,
    /// Store floats at half precision.
    pub half: bool,
}

impl PyCodec<'_> {
    fn attrs<S: Serializer>(&self, attrs: &PyAttrs, s: S) -> Result<S::Ok, S::Error> {
        let entries: Vec<(Bound<'_, PyAny>, Bound<'_, PyAny>)> = match attrs.dict(self.py) {
            Some(d) => d.iter().collect(),
            None => Vec::new(),
        };
        PyEntries { entries: &entries, half: self.half }.serialize(s)
    }
}

impl Codec<NodeData, EdgeData> for PyCodec<'_> {
    fn node_attr<S: Serializer>(&self, node: &NodeData, s: S) -> Result<S::Ok, S::Error> {
        self.attrs(&node.attr, s)
    }
    fn node_meta<S: Serializer>(&self, node: &NodeData, s: S) -> Result<S::Ok, S::Error> {
        self.attrs(&node.meta, s)
    }
    fn edge_attr<S: Serializer>(&self, edge: &EdgeData, s: S) -> Result<S::Ok, S::Error> {
        self.attrs(&edge.attr, s)
    }
    fn edge_meta<S: Serializer>(&self, edge: &EdgeData, s: S) -> Result<S::Ok, S::Error> {
        self.attrs(&edge.meta, s)
    }
    fn graph_meta<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let entries: Vec<(Bound<'_, PyAny>, Bound<'_, PyAny>)> = self.meta.iter().collect();
        PyEntries { entries: &entries, half: self.half }.serialize(s)
    }
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// String values up to this length are shared between all occurrences while
/// loading (e.g. an edge `type` repeated on 100k edges becomes one object).
const SHARED_STRING_MAX: usize = 64;

/// Interned short strings, keyed by their text in the input buffer.
pub type Strings<'s> = HashMap<&'s str, Py<PyAny>>;

/// Convert a loaded value to a Python object.
pub fn to_python<'s>(py: Python<'_>, value: &'s LoadValue<'_>, strings: &mut Strings<'s>) -> PyResult<Py<PyAny>> {
    match value.kind() {
        LoadKind::None => Ok(py.None()),
        LoadKind::String(s) => Ok(shared_string(py, s, strings)),
        LoadKind::Int(i) => Ok(i.into_pyobject(py)?.into_any().unbind()),
        LoadKind::Float(f) => Ok(f.into_pyobject(py)?.into_any().unbind()),
        LoadKind::Bool(b) => Ok(b.into_pyobject(py)?.to_owned().into_any().unbind()),
        LoadKind::List(items) => {
            let list = PyList::empty(py);
            for item in items {
                list.append(to_python(py, item, strings)?)?;
            }
            Ok(list.into_any().unbind())
        }
        LoadKind::Dict(entries) => {
            let dict = PyDict::new(py);
            for (key, item) in entries.iter() {
                dict.set_item(shared_string(py, key, strings), to_python(py, item, strings)?)?;
            }
            Ok(dict.into_any().unbind())
        }
    }
}

/// Convert a loaded attribute map (keys are shared like short strings).
pub fn to_attrs<'s>(py: Python<'_>, attrs: &'s LoadAttrs<'_>, strings: &mut Strings<'s>) -> PyResult<PyAttrs> {
    if attrs.is_empty() {
        return Ok(PyAttrs::default());
    }
    let dict = PyDict::new(py);
    for (key, value) in attrs.iter() {
        let key = shared_string(py, key, strings);
        dict.set_item(key, to_python(py, value, strings)?)?;
    }
    Ok(PyAttrs::from_dict(dict))
}

fn shared_string<'s>(py: Python<'_>, s: &'s str, strings: &mut Strings<'s>) -> Py<PyAny> {
    if s.len() > SHARED_STRING_MAX {
        return PyString::new(py, s).into_any().unbind();
    }
    strings.entry(s).or_insert_with(|| PyString::new(py, s).into_any().unbind()).clone_ref(py)
}
