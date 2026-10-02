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
use ironweaver_core::temporal::Parts;
use ironweaver_core::{Date, DateTime, Value};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{
    PyAny, PyBool, PyByteArray, PyBytes, PyDate, PyDateAccess, PyDateTime, PyDelta, PyDeltaAccess, PyDict, PyFloat,
    PyInt, PyList, PyMapping, PySequence, PyString, PyTimeAccess, PyTuple, PyTzInfo,
};
use serde::ser::{Error as _, SerializeMap, SerializeSeq, Serializer};
use serde::Serialize;
use std::collections::HashMap;

use crate::data::{EdgeData, NodeData, PyAttrs};

/// Dictionary keys are serialized as strings; non-string keys use `str(key)`.
fn dict_key(key: &Bound<'_, PyAny>) -> PyResult<String> {
    match key.cast::<PyString>() {
        Ok(s) => Ok(s.to_str()?.to_owned()),
        Err(_) => key.str()?.extract(),
    }
}

fn py_err<E: serde::ser::Error>(e: PyErr) -> E {
    ironweaver_core::format::ser_error(e)
}

/// A plain Python value (dict/list/str/number/bool/None) written as ordinary
/// JSON. Used to turn a graph *dict* into JSON bytes, which then go through
/// the same fast loader as JSON text.
struct PlainPy<'a, 'py>(&'a Bound<'py, PyAny>, usize);

/// Deepest nesting `PlainPy` writes. A graph dict needs a few levels for the
/// document and two per value level ({"List": [...]}); anything deeper is
/// beyond what the JSON loader accepts anyway.
const PLAIN_MAX_DEPTH: usize = 2 * ironweaver_core::format::MAX_DEPTH + 16;

impl Serialize for PlainPy<'_, '_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let (v, depth) = (self.0, self.1);
        if depth > PLAIN_MAX_DEPTH {
            return Err(S::Error::custom("graph dict nested too deeply (or containing itself)"));
        }
        if v.is_none() {
            s.serialize_unit()
        } else if let Ok(b) = v.cast::<PyBool>() {
            s.serialize_bool(b.is_true())
        } else if let Ok(i) = v.cast::<PyInt>() {
            match i.extract::<i64>() {
                Ok(n) => s.serialize_i64(n),
                Err(_) => s.serialize_f64(i.extract::<f64>().map_err(py_err)?),
            }
        } else if let Ok(f) = v.cast::<PyFloat>() {
            match f.value() {
                // As saved: JSON has no NaN or infinities
                f if f.is_nan() => s.serialize_str("NaN"),
                f if f == f64::INFINITY => s.serialize_str("Infinity"),
                f if f == f64::NEG_INFINITY => s.serialize_str("-Infinity"),
                f => s.serialize_f64(f),
            }
        } else if let Ok(st) = v.cast::<PyString>() {
            s.serialize_str(st.to_str().map_err(py_err)?)
        } else if let Ok(dict) = v.cast::<PyDict>() {
            let mut map = s.serialize_map(Some(dict.len()))?;
            for (k, val) in dict.iter() {
                map.serialize_entry(&dict_key(&k).map_err(py_err)?, &PlainPy(&val, depth + 1))?;
            }
            map.end()
        } else if let Ok(list) = v.cast::<PyList>() {
            let mut seq = s.serialize_seq(Some(list.len()))?;
            for item in list.iter() {
                seq.serialize_element(&PlainPy(&item, depth + 1))?;
            }
            seq.end()
        } else if let Ok(tuple) = v.cast::<PyTuple>() {
            let mut seq = s.serialize_seq(Some(tuple.len()))?;
            for item in tuple.iter() {
                seq.serialize_element(&PlainPy(&item, depth + 1))?;
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
    sonic_rs::to_vec(&PlainPy(obj, 0)).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Saving
// ---------------------------------------------------------------------------

/// A Python value serialized as a tagged value. Type dispatch matches the
/// loader's expectations: `bool` is checked before `int` (it is an `int`
/// subclass), numpy arrays and scalars go through one bulk `tolist()`,
/// anything else falls back to `str()`. `depth` is 1 for an attribute's
/// value (see `tagged::check_depth`).
struct PyValue<'a, 'py> {
    value: &'a Bound<'py, PyAny>,
    half: bool,
    depth: usize,
}

impl Serialize for PyValue<'_, '_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let (v, half, depth) = (self.value, self.half, self.depth);
        tagged::check_depth(depth)?;

        if v.is_none() {
            tagged::none(s)
        } else if let Ok(b) = v.cast::<PyBool>() {
            tagged::bool(s, b.is_true())
        } else if let Ok(i) = v.cast::<PyInt>() {
            match i.extract::<i64>() {
                Ok(n) => tagged::int(s, n),
                // Out of i64 range: keep the magnitude as a float
                Err(_) => tagged::float(s, i.extract::<f64>().map_err(py_err)?, half),
            }
        } else if let Ok(f) = v.cast::<PyFloat>() {
            tagged::float(s, f.value(), half)
        } else if let Ok(st) = v.cast::<PyString>() {
            tagged::string(s, st.to_str().map_err(py_err)?)
        } else if let Some(special) = special_value(v).map_err(py_err)? {
            match special {
                Value::Bytes(b) => tagged::bytes(s, &b),
                Value::Date(d) => tagged::date(s, d),
                Value::DateTime(t) => tagged::datetime(s, t),
                _ => unreachable!("special_value returns bytes, dates and date-times"),
            }
        } else if let Ok(list) = v.cast::<PyList>() {
            let items: Vec<Bound<'_, PyAny>> = list.iter().collect();
            tagged::list(s, &PyItems { items: &items, half, depth })
        } else if let Ok(tuple) = v.cast::<PyTuple>() {
            let items: Vec<Bound<'_, PyAny>> = tuple.iter().collect();
            tagged::list(s, &PyItems { items: &items, half, depth })
        } else if let Ok(dict) = v.cast::<PyDict>() {
            let entries: Vec<(Bound<'_, PyAny>, Bound<'_, PyAny>)> = dict.iter().collect();
            tagged::dict(s, &PyEntries { entries: &entries, half, depth })
        } else if v.hasattr("tolist").map_err(py_err)? {
            // numpy arrays and numpy scalars (e.g. embeddings)
            let native = v.call_method0("tolist").map_err(py_err)?;
            PyValue { value: &native, half, depth }.serialize(s)
        } else if let Ok(mapping) = v.cast::<PyMapping>() {
            let mut entries = Vec::new();
            for item in mapping.items().map_err(py_err)?.iter() {
                entries.push(item.extract::<(Bound<'_, PyAny>, Bound<'_, PyAny>)>().map_err(py_err)?);
            }
            tagged::dict(s, &PyEntries { entries: &entries, half, depth })
        } else if let Ok(seq) = v.cast::<PySequence>() {
            let mut items = Vec::new();
            for item in seq.try_iter().map_err(py_err)? {
                items.push(item.map_err(py_err)?);
            }
            tagged::list(s, &PyItems { items: &items, half, depth })
        } else {
            // Fallback: string representation
            let text = v.str().map_err(py_err)?;
            tagged::string(s, text.to_str().map_err(py_err)?)
        }
    }
}

/// Items of a list value at `depth`.
struct PyItems<'a, 'py> {
    items: &'a [Bound<'py, PyAny>],
    half: bool,
    depth: usize,
}

impl Serialize for PyItems<'_, '_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(self.items.len()))?;
        for item in self.items {
            seq.serialize_element(&PyValue { value: item, half: self.half, depth: self.depth + 1 })?;
        }
        seq.end()
    }
}

/// Entries of a dict value at `depth` (0 for an attribute map).
struct PyEntries<'a, 'py> {
    entries: &'a [(Bound<'py, PyAny>, Bound<'py, PyAny>)],
    half: bool,
    depth: usize,
}

impl Serialize for PyEntries<'_, '_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(self.entries.len()))?;
        for (k, v) in self.entries {
            let value = PyValue { value: v, half: self.half, depth: self.depth + 1 };
            match k.cast::<PyString>() {
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
        PyEntries { entries: &entries, half: self.half, depth: 0 }.serialize(s)
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
        PyEntries { entries: &entries, half: self.half, depth: 0 }.serialize(s)
    }
}

/// A Python `bytes` / `bytearray`, `datetime.datetime` or `datetime.date`
/// as a core value; `None` for anything else. An aware datetime keeps its
/// UTC offset (a fixed offset: zone names are not kept); offsets with
/// microseconds are refused.
pub fn special_value(v: &Bound<'_, PyAny>) -> PyResult<Option<Value>> {
    Ok(Some(if let Ok(b) = v.cast::<PyBytes>() {
        Value::Bytes(b.as_bytes().to_vec())
    } else if let Ok(b) = v.cast::<PyByteArray>() {
        Value::Bytes(b.to_vec())
    } else if let Ok(t) = v.cast::<PyDateTime>() {
        let offset = match v.call_method0("utcoffset")? {
            o if o.is_none() => None,
            o => {
                let d = o.cast_into::<PyDelta>()?;
                if d.get_microseconds() != 0 {
                    return Err(PyValueError::new_err("UTC offsets with microseconds are not supported"));
                }
                Some(d.get_days() * 86_400 + d.get_seconds())
            }
        };
        let parts = Parts {
            year: t.get_year(),
            month: t.get_month().into(),
            day: t.get_day().into(),
            hour: t.get_hour().into(),
            minute: t.get_minute().into(),
            second: t.get_second().into(),
            microsecond: t.get_microsecond(),
        };
        Value::DateTime(DateTime::from_parts(parts, offset).map_err(crate::errors::graph_error)?)
    } else if let Ok(d) = v.cast::<PyDate>() {
        Value::Date(
            Date::from_ymd(d.get_year(), d.get_month().into(), d.get_day().into())
                .map_err(crate::errors::graph_error)?,
        )
    } else {
        return Ok(None);
    }))
}

/// A core bytes / date / date-time value as a Python object.
fn special_to_python(py: Python<'_>, v: &LoadKind<'_, '_>) -> PyResult<Py<PyAny>> {
    Ok(match v {
        LoadKind::Bytes(b) => PyBytes::new(py, b).into_any().unbind(),
        LoadKind::Date(d) => {
            let (y, m, day) = d.ymd();
            PyDate::new(py, y, m as u8, day as u8)?.into_any().unbind()
        }
        LoadKind::DateTime(t) => {
            let p = t.parts();
            let tz = t.offset.map(|o| PyTzInfo::fixed_offset(py, PyDelta::new(py, 0, o, 0, true)?)).transpose()?;
            let (mo, d, h, mi, s) = (p.month as u8, p.day as u8, p.hour as u8, p.minute as u8, p.second as u8);
            PyDateTime::new(py, p.year, mo, d, h, mi, s, p.microsecond, tz.as_ref())?.into_any().unbind()
        }
        _ => unreachable!("only called for bytes, dates and date-times"),
    })
}

/// A Python value as a core `Value` (for filter expressions): None, bool,
/// int (float if out of i64 range), float, str, bytes, date, datetime,
/// list / tuple, dict (keys as strings); anything else as `str(value)`.
/// Fails beyond `MAX_DEPTH` levels.
pub fn to_value(v: &Bound<'_, PyAny>) -> PyResult<ironweaver_core::Value> {
    fn go(v: &Bound<'_, PyAny>, depth: usize) -> PyResult<ironweaver_core::Value> {
        if depth > ironweaver_core::format::MAX_DEPTH {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "values nested more than {} levels deep",
                ironweaver_core::format::MAX_DEPTH
            )));
        }
        Ok(if v.is_none() {
            Value::None
        } else if let Ok(b) = v.cast::<PyBool>() {
            Value::Bool(b.is_true())
        } else if let Ok(i) = v.cast::<PyInt>() {
            match i.extract::<i64>() {
                Ok(n) => Value::Int(n),
                Err(_) => Value::Float(i.extract::<f64>()?),
            }
        } else if let Ok(f) = v.cast::<PyFloat>() {
            Value::Float(f.value())
        } else if let Ok(s) = v.cast::<PyString>() {
            Value::String(s.to_str()?.to_owned())
        } else if let Some(special) = special_value(v)? {
            special
        } else if let Ok(d) = v.cast::<PyDict>() {
            let mut out = HashMap::with_capacity(d.len());
            for (k, x) in d.iter() {
                out.insert(dict_key(&k)?, go(&x, depth + 1)?);
            }
            Value::Dict(out)
        } else if let Ok(l) = v.cast::<PyList>() {
            Value::List(l.iter().map(|x| go(&x, depth + 1)).collect::<PyResult<_>>()?)
        } else if let Ok(t) = v.cast::<PyTuple>() {
            Value::List(t.iter().map(|x| go(&x, depth + 1)).collect::<PyResult<_>>()?)
        } else {
            Value::String(v.str()?.to_str()?.to_owned())
        })
    }
    go(v, 1)
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
        kind @ (LoadKind::Bytes(_) | LoadKind::Date(_) | LoadKind::DateTime(_)) => special_to_python(py, &kind),
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
