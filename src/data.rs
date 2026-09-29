// data.rs
//
// Node and edge payloads of the Python-facing graph. Attribute and meta maps
// are Python dicts, shared copy-on-write between a graph and the graphs
// derived from it (filter, traversals, paths): deriving only increments a
// reference count, and a dict is copied before it is changed while another
// graph still holds it. Dicts holding only atomic values are not tracked by
// Python's cyclic GC.

use ironweaver_core::{Attributes, Graph, Lookup};
use pyo3::conversion::FromPyObjectOwned;
use pyo3::exceptions::{PyKeyError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyString};
use std::cell::RefCell;
use std::collections::HashMap;

/// Attribute filters passed from Python (`{key: expected_value}`).
pub type AttrMap = HashMap<String, Py<PyAny>>;

/// The graph type behind every `Vertex`.
pub type PyGraph = Graph<NodeData, EdgeData>;

/// An `attr` / `meta` dict (never handed out: getters return copies).
/// `None` until something is stored, which saves a dict per node for the
/// usually empty `meta`. The dict may be shared with derived graphs; change
/// it only through `unique` / `unique_now`.
#[derive(Default)]
pub struct PyAttrs(Option<Py<PyDict>>);

/// Reference count of `d`; 1 means only this `PyAttrs` holds it.
fn refcnt(_py: Python<'_>, d: &Py<PyDict>) -> isize {
    // SAFETY: `d` is a live object and the GIL is held (`_py`)
    unsafe { pyo3::ffi::Py_REFCNT(d.as_ptr()) }
}

impl PyAttrs {
    /// A copy of a user-supplied dict; keys must be strings.
    pub fn from_user(dict: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        let dict = match dict {
            Some(d) => d,
            None => return Ok(PyAttrs(None)),
        };
        for key in dict.keys() {
            if !key.is_instance_of::<PyString>() {
                return Err(PyTypeError::new_err(format!(
                    "attribute keys must be strings, got {}",
                    key.get_type().name()?
                )));
            }
        }
        Ok(PyAttrs(Some(dict.copy()?.unbind())))
    }

    /// Take ownership of a dict built by the library (no copy, no checks).
    pub fn from_dict(dict: Bound<'_, PyDict>) -> Self {
        if dict.is_empty() {
            PyAttrs(None)
        } else {
            PyAttrs(Some(dict.unbind()))
        }
    }

    pub fn dict<'py>(&self, py: Python<'py>) -> Option<&Bound<'py, PyDict>> {
        self.0.as_ref().map(|d| d.bind(py))
    }

    /// The value under `key`, if any.
    pub fn get<'py>(&self, py: Python<'py>, key: &str) -> PyResult<Option<Bound<'py, PyAny>>> {
        match self.dict(py) {
            Some(d) => d.get_item(cached_key(py, key)),
            None => Ok(None),
        }
    }

    /// The dict, if it exists and no other graph shares it (safe to change
    /// in place right now).
    pub fn unique_now(&self, py: Python<'_>) -> Option<&Py<PyDict>> {
        self.0.as_ref().filter(|d| refcnt(py, d) == 1)
    }

    /// The dict, ready to be changed in place: created if missing, copied
    /// first if another graph shares it.
    pub fn unique(&mut self, py: Python<'_>) -> PyResult<&Py<PyDict>> {
        let shared = match &self.0 {
            Some(d) => refcnt(py, d) > 1,
            None => true,
        };
        if shared {
            let dict = match self.dict(py) {
                Some(d) => d.copy()?,
                None => PyDict::new(py),
            };
            self.0 = Some(dict.unbind());
        }
        Ok(self.0.as_ref().expect("set above"))
    }

    /// The same contents for a derived graph: shares the dict (copy-on-write).
    pub fn share(&self, py: Python<'_>) -> Self {
        PyAttrs(self.0.as_ref().map(|d| d.clone_ref(py)))
    }

    /// A new dict with the contents, for handing to Python.
    pub fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        match self.dict(py) {
            Some(d) => d.copy(),
            None => Ok(PyDict::new(py)),
        }
    }

    pub fn py(&self) -> Option<&Py<PyDict>> {
        self.0.as_ref()
    }
}

thread_local! {
    /// Python strings for attribute keys looked up by the core algorithms
    /// (edge weights, coordinates, edge types), so hot loops don't create a
    /// new string object per lookup.
    static KEYS: RefCell<Vec<(String, Py<PyString>)>> = const { RefCell::new(Vec::new()) };
}

const MAX_CACHED_KEYS: usize = 64;

fn cached_key<'py>(py: Python<'py>, key: &str) -> Bound<'py, PyString> {
    KEYS.with(|keys| {
        let mut keys = keys.borrow_mut();
        if let Some((_, k)) = keys.iter().find(|(s, _)| s == key) {
            return k.bind(py).clone();
        }
        if keys.len() >= MAX_CACHED_KEYS {
            keys.clear();
        }
        let k = PyString::intern(py, key);
        keys.push((key.to_owned(), k.clone().unbind()));
        k
    })
}

/// A list of Python objects that is almost always empty (16 bytes, no
/// allocation when empty).
#[derive(Default)]
pub struct PyObjects(Option<Box<[Py<PyAny>]>>);

impl PyObjects {
    pub fn to_vec(&self, py: Python<'_>) -> Vec<Py<PyAny>> {
        self.iter().map(|o| o.clone_ref(py)).collect()
    }

    pub fn from_vec(items: Vec<Py<PyAny>>) -> Self {
        PyObjects(if items.is_empty() { None } else { Some(items.into_boxed_slice()) })
    }

    pub fn iter(&self) -> impl Iterator<Item = &Py<PyAny>> {
        self.0.iter().flat_map(|v| v.iter())
    }
}

pub struct NodeData {
    pub attr: PyAttrs,
    pub meta: PyAttrs,
    pub on_edge_add_callbacks: PyObjects,
}

pub struct EdgeData {
    pub attr: PyAttrs,
    pub meta: PyAttrs,
    /// Rarely set fields, boxed to keep edges small.
    pub extra: Option<Box<EdgeExtra>>,
}

#[derive(Default)]
pub struct EdgeExtra {
    /// Id read from a saved graph (`None` for edges made with `add_edge`).
    pub id: Option<Box<str>>,
    pub watched_by: PyObjects,
    pub on_meta_change_callbacks: PyObjects,
}

impl NodeData {
    pub fn new(attr: PyAttrs, meta: PyAttrs) -> Self {
        NodeData { attr, meta, on_edge_add_callbacks: PyObjects::default() }
    }

    /// Copy for a derived graph: `attr` / `meta` hold the same contents
    /// (copy-on-write), callback lists start empty.
    pub fn copy(&self, py: Python<'_>) -> PyResult<Self> {
        Ok(NodeData::new(self.attr.share(py), self.meta.share(py)))
    }

    pub fn visit(&self, visit: &mut dyn FnMut(&Py<PyAny>)) {
        for d in [&self.attr, &self.meta].into_iter().filter_map(PyAttrs::py) {
            visit(d.as_any());
        }
        self.on_edge_add_callbacks.iter().for_each(visit);
    }
}

impl EdgeData {
    pub fn new(id: Option<Box<str>>, attr: PyAttrs, meta: PyAttrs) -> Self {
        let extra = id.map(|id| Box::new(EdgeExtra { id: Some(id), ..Default::default() }));
        EdgeData { attr, meta, extra }
    }

    pub fn id(&self) -> Option<&str> {
        self.extra.as_ref().and_then(|x| x.id.as_deref())
    }

    /// The rarely set fields, created on first use.
    pub fn extra_mut(&mut self) -> &mut EdgeExtra {
        self.extra.get_or_insert_with(Default::default)
    }

    pub fn watched_by(&self) -> Option<&PyObjects> {
        self.extra.as_ref().map(|x| &x.watched_by)
    }

    pub fn on_meta_change_callbacks(&self) -> Option<&PyObjects> {
        self.extra.as_ref().map(|x| &x.on_meta_change_callbacks)
    }

    /// Copy for a derived graph (see `NodeData::copy`); the id is kept.
    pub fn copy(&self, py: Python<'_>) -> PyResult<Self> {
        Ok(EdgeData::new(self.id().map(Into::into), self.attr.share(py), self.meta.share(py)))
    }

    pub fn visit(&self, visit: &mut dyn FnMut(&Py<PyAny>)) {
        for d in [&self.attr, &self.meta].into_iter().filter_map(PyAttrs::py) {
            visit(d.as_any());
        }
        if let Some(x) = &self.extra {
            x.watched_by.iter().chain(x.on_meta_change_callbacks.iter()).for_each(visit);
        }
    }
}

/// Follow `path` from `attr`: an attribute name, then keys looked up with
/// `[]` (dicts, or anything subscriptable). `None` if a step is missing or
/// the value is `None`.
fn lookup<'py>(py: Python<'py>, attr: &PyAttrs, path: &[String]) -> PyResult<Option<Bound<'py, PyAny>>> {
    let (first, rest) = match path.split_first() {
        Some(p) => p,
        None => return Ok(None),
    };
    let mut value = match attr.get(py, first)? {
        Some(v) => v,
        None => return Ok(None),
    };
    for key in rest {
        if value.is_none() {
            return Ok(None);
        }
        value = if let Ok(d) = value.cast::<PyDict>() {
            match d.get_item(cached_key(py, key))? {
                Some(v) => v,
                None => return Ok(None),
            }
        } else {
            match value.get_item(key) {
                Ok(v) => v,
                Err(e) if e.is_instance_of::<PyKeyError>(py) => return Ok(None),
                Err(e) => return Err(e),
            }
        };
    }
    Ok(if value.is_none() { None } else { Some(value) })
}

fn extracted<'py, T: FromPyObjectOwned<'py>>(value: Option<Bound<'py, PyAny>>) -> Lookup<T> {
    match value {
        None => Lookup::Missing,
        Some(v) => v.extract::<T>().map_or(Lookup::Invalid, Lookup::Found),
    }
}

// Attribute reads for the core algorithms. They run while the caller holds
// the GIL, so `attach` only fetches the token.

fn number(attr: &PyAttrs, path: &[String]) -> PyResult<Lookup<f64>> {
    Python::attach(|py| Ok(extracted(lookup(py, attr, path)?)))
}

fn numbers(attr: &PyAttrs, path: &[String]) -> PyResult<Lookup<Vec<f64>>> {
    Python::attach(|py| Ok(extracted(lookup(py, attr, path)?)))
}

fn text(attr: &PyAttrs, key: &str) -> PyResult<Lookup<String>> {
    Python::attach(|py| {
        Ok(match attr.get(py, key)? {
            None => Lookup::Missing,
            Some(v) if v.is_none() => Lookup::Missing,
            Some(v) => v.extract::<String>().map_or(Lookup::Invalid, Lookup::Found),
        })
    })
}

impl Attributes for NodeData {
    type Error = PyErr;

    fn number(&self, path: &[String]) -> PyResult<Lookup<f64>> {
        number(&self.attr, path)
    }
    fn numbers(&self, path: &[String]) -> PyResult<Lookup<Vec<f64>>> {
        numbers(&self.attr, path)
    }
    fn text(&self, key: &str) -> PyResult<Lookup<String>> {
        text(&self.attr, key)
    }
}

impl Attributes for EdgeData {
    type Error = PyErr;

    fn number(&self, path: &[String]) -> PyResult<Lookup<f64>> {
        number(&self.attr, path)
    }
    fn numbers(&self, path: &[String]) -> PyResult<Lookup<Vec<f64>>> {
        numbers(&self.attr, path)
    }
    fn text(&self, key: &str) -> PyResult<Lookup<String>> {
        text(&self.attr, key)
    }
}
