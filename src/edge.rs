// edge.rs

use ironweaver_core::EdgeIx;
use pyo3::class::basic::CompareOp;
use pyo3::exceptions::{PyRuntimeError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList, PyTuple};
use pyo3::{PyTraverseError, PyVisit};
use std::hash::{Hash, Hasher};

use crate::data::{EdgeData, PyAttrs, PyObjects};
use crate::vertex::callbacks::fire;
use crate::{Node, Vertex};

/// A directed edge from `from_node` to `to_node` with an `attr` dict.
///
/// An `Edge` is a handle to an edge stored in its `vertex`; two handles to
/// the same edge compare equal. Create edges with `Vertex.add_edge`. `attr`
/// and `meta` return copies: use `attr_set` / `attr_get` to change or read
/// single attributes.
#[pyclass(frozen)]
pub struct Edge {
    pub(crate) vertex: Py<Vertex>,
    pub(crate) ix: EdgeIx,
}

fn stale() -> PyErr {
    PyRuntimeError::new_err("edge was removed from its graph")
}

impl Edge {
    pub fn handle(py: Python<'_>, vertex: &Py<Vertex>, ix: EdgeIx) -> PyResult<Py<Edge>> {
        Py::new(py, Edge { vertex: vertex.clone_ref(py), ix })
    }

    fn read<R>(&self, py: Python<'_>, f: impl FnOnce(&ironweaver_core::Edge<EdgeData>) -> R) -> PyResult<R> {
        let v = self.vertex.try_borrow(py)?;
        let edge = v.graph.edge(self.ix).ok_or_else(stale)?;
        Ok(f(edge))
    }

    fn write<R>(&self, py: Python<'_>, f: impl FnOnce(&mut ironweaver_core::Edge<EdgeData>) -> R) -> PyResult<R> {
        let mut v = self.vertex.try_borrow_mut(py)?;
        let edge = v.graph.edge_mut(self.ix).ok_or_else(stale)?;
        Ok(f(edge))
    }

    /// The edge's `attr` dict (created if missing); see `Node::attr_dict`.
    fn attr_dict(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        if let Some(d) = self.read(py, |e| e.data.attr.unique_now(py).map(|d| d.clone_ref(py)))? {
            return Ok(d);
        }
        self.write(py, |e| e.data.attr.unique(py).map(|d| d.clone_ref(py)))?
    }
}

#[pymethods]
impl Edge {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        Err(PyTypeError::new_err("Edges are created with Vertex.add_edge(from_id, to_id, attr)"))
    }

    fn __repr__(&self, py: Python<'_>) -> String {
        let v = match self.vertex.try_borrow(py) {
            Ok(v) => v,
            Err(_) => return "<edge>".to_string(),
        };
        let edge = match v.graph.edge(self.ix) {
            Some(e) => e,
            None => return "<removed edge>".to_string(),
        };
        let typ = edge
            .data
            .attr
            .get(py, "type")
            .ok()
            .flatten()
            .and_then(|t| t.extract::<String>().ok())
            .unwrap_or_else(|| "unknown".to_string());
        let id = |n| v.graph.node(n).map_or("?", |n| n.id());
        format!("{}: {} --> {}", typ, id(edge.source()), id(edge.target()))
    }

    fn __richcmp__(&self, other: &Bound<'_, PyAny>, op: CompareOp) -> PyObject {
        let py = other.py();
        let same = match other.downcast::<Edge>() {
            Ok(o) => {
                let o = o.get();
                o.vertex.is(&self.vertex) && o.ix == self.ix
            }
            Err(_) => return py.NotImplemented(),
        };
        match op {
            CompareOp::Eq => same.into_pyobject(py).unwrap().to_owned().into_any().unbind(),
            CompareOp::Ne => (!same).into_pyobject(py).unwrap().to_owned().into_any().unbind(),
            _ => py.NotImplemented(),
        }
    }

    fn __hash__(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (self.vertex.as_ptr() as usize, self.ix).hash(&mut h);
        h.finish()
    }

    // Garbage-collector support: an edge keeps its Vertex alive.
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.vertex)
    }

    /// The id the edge was saved under (None for edges made with `add_edge`).
    #[getter]
    fn id(&self, py: Python<'_>) -> PyResult<Option<String>> {
        self.read(py, |e| e.data.id().map(str::to_owned))
    }

    #[setter]
    fn set_id(&self, py: Python<'_>, id: Option<String>) -> PyResult<()> {
        self.write(py, |e| e.data.extra_mut().id = id.map(String::into_boxed_str))
    }

    #[getter]
    #[allow(clippy::wrong_self_convention)]
    fn from_node(&self, py: Python<'_>) -> PyResult<Py<Node>> {
        let ix = self.read(py, |e| e.source())?;
        Node::handle(py, &self.vertex, ix)
    }

    #[getter]
    fn to_node(&self, py: Python<'_>) -> PyResult<Py<Node>> {
        let ix = self.read(py, |e| e.target())?;
        Node::handle(py, &self.vertex, ix)
    }

    /// A copy of the attributes.
    #[getter]
    fn attr<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        self.read(py, |e| e.data.attr.to_dict(py))?
    }

    #[setter]
    fn set_attr(&self, py: Python<'_>, attr: Bound<'_, PyDict>) -> PyResult<()> {
        let attr = PyAttrs::from_user(Some(&attr))?;
        let old = self.write(py, |e| std::mem::replace(&mut e.data.attr, attr))?;
        drop(old); // after the borrow ends, in case a value's __del__ touches the graph
        Ok(())
    }

    /// A copy of the edge's metadata.
    #[getter]
    fn meta<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        self.read(py, |e| e.data.meta.to_dict(py))?
    }

    #[setter]
    fn set_meta(&self, py: Python<'_>, meta: Bound<'_, PyDict>) -> PyResult<()> {
        let meta = PyAttrs::from_user(Some(&meta))?;
        let old = self.write(py, |e| std::mem::replace(&mut e.data.meta, meta))?;
        drop(old);
        Ok(())
    }

    #[getter]
    fn watched_by(&self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        self.read(py, |e| e.data.watched_by().map_or_else(Vec::new, |w| w.to_vec(py)))
    }

    #[setter]
    fn set_watched_by(&self, py: Python<'_>, watchers: Vec<Py<PyAny>>) -> PyResult<()> {
        let watchers = PyObjects::from_vec(watchers);
        let old = self.write(py, |e| std::mem::replace(&mut e.data.extra_mut().watched_by, watchers))?;
        drop(old);
        Ok(())
    }

    #[getter]
    fn on_meta_change_callbacks(&self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        self.read(py, |e| e.data.on_meta_change_callbacks().map_or_else(Vec::new, |c| c.to_vec(py)))
    }

    #[setter]
    fn set_on_meta_change_callbacks(&self, py: Python<'_>, callbacks: Vec<Py<PyAny>>) -> PyResult<()> {
        let callbacks = PyObjects::from_vec(callbacks);
        let old = self.write(py, |e| std::mem::replace(&mut e.data.extra_mut().on_meta_change_callbacks, callbacks))?;
        drop(old);
        Ok(())
    }

    /// The owning Vertex's `on_edge_update_callbacks` (a live list).
    #[getter]
    fn on_update_callbacks(&self, py: Python<'_>) -> PyResult<Py<PyList>> {
        Ok(self.vertex.try_borrow(py)?.on_edge_update_callbacks.clone_ref(py))
    }

    /// The Vertex this edge belongs to.
    #[getter]
    fn vertex(&self, py: Python<'_>) -> Py<Vertex> {
        self.vertex.clone_ref(py)
    }

    /// Return the edge's attributes as a dict (for JSON encoders).
    #[allow(non_snake_case)]
    fn toJSON<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        self.attr(py)
    }

    /// Set a value in ``attr`` under ``key``.
    /// Fires the vertex's ``on_edge_update_callbacks`` if the value changed.
    fn attr_set(slf: &Bound<'_, Self>, key: String, value: Py<PyAny>) -> PyResult<()> {
        let py = slf.py();
        let this = slf.get();
        let old = this.read(py, |e| e.data.attr.get(py, &key))??;
        let changed = match &old {
            Some(o) => !o.rich_compare(value.bind(py), CompareOp::Eq)?.is_truthy()?,
            None => true,
        };
        // Fetch the (unshared) dict only now: the comparison above may have
        // run Python code that derived a graph sharing it.
        this.attr_dict(py)?.bind(py).set_item(&key, &value)?;
        let old = old.map(Bound::unbind);

        if changed {
            let callbacks = this.vertex.try_borrow(py)?.on_edge_update_callbacks.clone_ref(py);
            let args = PyTuple::new(
                py,
                [
                    this.vertex.clone_ref(py).into_any(),
                    slf.clone().into_any().unbind(),
                    key.into_pyobject(py)?.into_any().unbind(),
                    value,
                    old.unwrap_or_else(|| py.None()),
                ],
            )?;
            fire(callbacks.bind(py), args)?;
        }
        Ok(())
    }

    /// Retrieve a value from ``attr`` by key.
    /// Returns ``None`` if the key does not exist.
    fn attr_get(&self, py: Python<'_>, key: &str) -> PyResult<Option<Py<PyAny>>> {
        Ok(self.read(py, |e| e.data.attr.get(py, key))??.map(Bound::unbind))
    }
}
