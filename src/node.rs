// node.rs

use ironweaver_core::traversal;
use ironweaver_core::{Direction, NodeIx};
use pyo3::class::basic::CompareOp;
use pyo3::exceptions::{PyRuntimeError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList, PyString, PyTuple};
use pyo3::{PyTraverseError, PyVisit};
use std::hash::{Hash, Hasher};

use crate::data::{node_value, AttrMap, EdgeData, NodeData, PyAttrs, PyGraph, PyObjects};
use crate::errors::Error;
use crate::interrupt;
use crate::vertex::callbacks::fire;
use crate::vertex::subgraph::build_subgraph;
use crate::{Edge, Vertex};

/// A graph node: an `id`, an `attr` dict, outgoing `edges` and incoming
/// `inverse_edges`.
///
/// A `Node` is a handle to a node stored in its `vertex`; two handles to the
/// same node compare equal. Create nodes with `Vertex.add_node`. The `attr`,
/// `meta`, `edges` and `inverse_edges` properties return copies:
/// `node.attr["k"] = v` has no effect. Use `attr_set` / `attr_get` (which
/// fire the owning Vertex's update callbacks) or assign a whole dict
/// (`node.attr = {...}`).
///
/// Traversals start here: `bfs`, `traverse` (DFS) and `bfs_search`.
#[pyclass(frozen)]
pub struct Node {
    pub(crate) vertex: Py<Vertex>,
    pub(crate) ix: NodeIx,
}

fn stale() -> PyErr {
    PyRuntimeError::new_err("node was removed from its graph")
}

impl Node {
    pub fn handle(py: Python<'_>, vertex: &Py<Vertex>, ix: NodeIx) -> PyResult<Py<Node>> {
        Py::new(py, Node { vertex: vertex.clone_ref(py), ix })
    }

    /// Run `f` on the node's data (the vertex is borrowed meanwhile).
    fn read<R>(&self, py: Python<'_>, f: impl FnOnce(&ironweaver_core::Node<NodeData>) -> R) -> PyResult<R> {
        let v = self.vertex.try_borrow(py)?;
        let node = v.graph.node(self.ix).ok_or_else(stale)?;
        Ok(f(node))
    }

    /// Run `f` on the node's data mutably.
    fn write<R>(&self, py: Python<'_>, f: impl FnOnce(&mut ironweaver_core::Node<NodeData>) -> R) -> PyResult<R> {
        let mut v = self.vertex.try_borrow_mut(py)?;
        let node = v.graph.node_mut(self.ix).ok_or_else(stale)?;
        Ok(f(node))
    }

    /// The node's `attr` dict (created if missing), for changing it without
    /// holding a borrow of the vertex while Python code runs.
    fn attr_dict(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        if let Some(d) = self.read(py, |n| n.data.attr.unique_now(py).map(|d| d.clone_ref(py)))? {
            return Ok(d);
        }
        self.write(py, |n| n.data.attr.unique(py).map(|d| d.clone_ref(py)))?
    }

    fn edge_handles(&self, py: Python<'_>, incoming: bool) -> PyResult<Vec<Py<Edge>>> {
        let edges = self.read(py, |n| if incoming { n.in_edges().to_vec() } else { n.out_edges().to_vec() })?;
        edges.into_iter().map(|e| Edge::handle(py, &self.vertex, e)).collect()
    }

    /// Visiting order of a DFS or BFS from this node, as a new Vertex (copies
    /// of the reached nodes and the edges between them).
    fn walk(
        &self,
        py: Python<'_>,
        depth: Option<usize>,
        filter: Option<AttrMap>,
        edge_filter: Option<Py<PyAny>>,
        breadth_first: bool,
    ) -> PyResult<Py<Vertex>> {
        let v = self.vertex.try_borrow(py)?;
        v.graph.node(self.ix).ok_or_else(stale)?;
        let ok = edge_predicate(py, &self.vertex, &filter, &edge_filter);
        let order = if breadth_first {
            interrupt::polling(py, || traversal::bfs(&v.graph, self.ix, depth, ok))??
        } else {
            interrupt::polling(py, || traversal::dfs(&v.graph, self.ix, depth, ok))??
        };
        let meta = PyDict::new(py);
        let ids: Vec<&str> = order.iter().map(|&n| v.graph.node(n).expect("visited nodes are live").id()).collect();
        meta.set_item("nodelist", ids)?;
        build_subgraph(py, &v, order, meta.unbind(), false)
    }
}

/// Edge filter for traversals: every `filter` pair must equal the edge's
/// attribute (Python `==`), and `edge_filter(edge)` must return True.
pub(crate) fn edge_predicate<'a>(
    py: Python<'a>,
    vertex: &'a Py<Vertex>,
    filter: &'a Option<AttrMap>,
    edge_filter: &'a Option<Py<PyAny>>,
) -> impl FnMut(ironweaver_core::EdgeIx, &ironweaver_core::Edge<EdgeData>) -> Result<bool, Error> + 'a {
    // Interned keys: every lookup then hits the dict's pointer fast path.
    let wanted: Vec<(Bound<'a, PyString>, Bound<'a, PyAny>)> =
        filter.iter().flatten().map(|(k, v)| (PyString::intern(py, k), v.bind(py).clone())).collect();
    move |e, edge| {
        if !wanted.is_empty() {
            for (key, expected) in &wanted {
                // "type" is the edge's type field
                let value = if key.to_str()? == "type" {
                    let v = vertex.bind(py).try_borrow().map_err(PyErr::from)?;
                    v.graph.edge_type_name(e).map(|t| PyString::new(py, t).into_any())
                } else {
                    match edge.data.attr.dict(py) {
                        Some(d) => d.get_item(key)?,
                        None => None,
                    }
                };
                match value {
                    Some(value) if value.eq(expected)? => {}
                    _ => return Ok(false),
                }
            }
        }
        if let Some(callable) = edge_filter {
            let result = callable.call1(py, (Edge::handle(py, vertex, e)?,))?;
            if !result.extract::<bool>(py)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[pymethods]
impl Node {
    /// A standalone node, in a new one-node Vertex (`node.vertex`). Edges
    /// are made with `Vertex.add_edge`.
    #[new]
    #[pyo3(signature = (id, attr=None, edges=None))]
    fn new(
        py: Python<'_>,
        id: String,
        attr: Option<Bound<'_, PyDict>>,
        edges: Option<Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        if let Some(edges) = edges {
            if !edges.is_none() && edges.len()? > 0 {
                return Err(PyTypeError::new_err("Node edges are created with Vertex.add_edge(from_id, to_id, attr)"));
            }
        }
        let mut graph = PyGraph::new();
        let data = NodeData::new(PyAttrs::from_user(attr.as_ref())?, PyAttrs::default());
        let ix = graph.add_node(id, data).expect("empty graph");
        let vertex = Py::new(py, Vertex::with_graph(py, graph))?;
        Ok(Node { vertex, ix })
    }

    fn __repr__(&self, py: Python<'_>) -> String {
        self.read(py, |n| n.id().to_string()).unwrap_or_else(|_| "<removed node>".to_string())
    }

    fn __richcmp__(&self, other: &Bound<'_, PyAny>, op: CompareOp) -> Py<PyAny> {
        let py = other.py();
        let same = match other.cast::<Node>() {
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

    // Garbage-collector support: a node keeps its Vertex alive.
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.vertex)
    }

    /// The node id (unique within its vertex). Assigning renames the node.
    #[getter]
    fn id(&self, py: Python<'_>) -> PyResult<String> {
        self.read(py, |n| n.id().to_string())
    }

    #[setter]
    fn set_id(&self, py: Python<'_>, id: String) -> PyResult<()> {
        let mut v = self.vertex.try_borrow_mut(py)?;
        v.graph.rename_node(self.ix, id).map_err(crate::errors::graph_error)
    }

    /// A copy of the attributes, with the node's labels under "labels" (if
    /// it has any).
    #[getter]
    fn attr<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let d = self.read(py, |n| n.data.attr.to_dict(py))??;
        let labels = self.labels(py)?;
        if !labels.is_empty() {
            d.set_item("labels", labels)?;
        }
        Ok(d)
    }

    /// The node's labels (a new list). Assigning replaces them.
    #[getter]
    fn labels(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        let v = self.vertex.try_borrow(py)?;
        let labels = v.graph.label_names(self.ix).ok_or_else(stale)?;
        Ok(labels.into_iter().map(str::to_owned).collect())
    }

    #[setter]
    fn set_labels(&self, py: Python<'_>, labels: Vec<String>) -> PyResult<()> {
        let mut v = self.vertex.try_borrow_mut(py)?;
        let old: Vec<String> = v.graph.label_names(self.ix).ok_or_else(stale)?.into_iter().map(str::to_owned).collect();
        for l in old.iter().filter(|l| !labels.contains(l)) {
            v.graph.remove_label(self.ix, l).map_err(crate::errors::graph_error)?;
        }
        for l in &labels {
            v.graph.add_label(self.ix, l).map_err(crate::errors::graph_error)?;
        }
        Ok(())
    }

    /// Add a label; returns False if the node already had it.
    fn add_label(&self, py: Python<'_>, label: &str) -> PyResult<bool> {
        let mut v = self.vertex.try_borrow_mut(py)?;
        v.graph.add_label(self.ix, label).map_err(crate::errors::graph_error)
    }

    /// Remove a label; returns False if the node didn't have it.
    fn remove_label(&self, py: Python<'_>, label: &str) -> PyResult<bool> {
        let mut v = self.vertex.try_borrow_mut(py)?;
        v.graph.remove_label(self.ix, label).map_err(crate::errors::graph_error)
    }

    fn has_label(&self, py: Python<'_>, label: &str) -> PyResult<bool> {
        let v = self.vertex.try_borrow(py)?;
        let node = v.graph.node(self.ix).ok_or_else(stale)?;
        Ok(v.graph.symbol(label).is_some_and(|s| node.has_label(s)))
    }

    /// Replace the attributes; a list of str under "labels" sets the node's
    /// labels (no "labels" clears them).
    #[setter]
    fn set_attr(&self, py: Python<'_>, attr: Bound<'_, PyDict>) -> PyResult<()> {
        let (attr, labels) = crate::data::split_reserved(Some(&attr), "labels")?;
        let labels: Vec<String> = labels.map(|l| l.extract()).transpose()?.unwrap_or_default();
        let old = self.write(py, |n| std::mem::replace(&mut n.data.attr, attr))?;
        drop(old); // after the borrow ends, in case a value's __del__ touches the graph
        self.set_labels(py, labels)
    }

    /// A copy of the node's metadata.
    #[getter]
    fn meta<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        self.read(py, |n| n.data.meta.to_dict(py))?
    }

    #[setter]
    fn set_meta(&self, py: Python<'_>, meta: Bound<'_, PyDict>) -> PyResult<()> {
        let meta = PyAttrs::from_user(Some(&meta))?;
        let old = self.write(py, |n| std::mem::replace(&mut n.data.meta, meta))?;
        drop(old);
        Ok(())
    }

    /// Outgoing edges (a new list).
    #[getter]
    fn edges(&self, py: Python<'_>) -> PyResult<Vec<Py<Edge>>> {
        self.edge_handles(py, false)
    }

    /// Incoming edges (a new list).
    #[getter]
    fn inverse_edges(&self, py: Python<'_>) -> PyResult<Vec<Py<Edge>>> {
        self.edge_handles(py, true)
    }

    #[getter]
    fn on_edge_add_callbacks(&self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        self.read(py, |n| n.data.on_edge_add_callbacks.to_vec(py))
    }

    #[setter]
    fn set_on_edge_add_callbacks(&self, py: Python<'_>, callbacks: Vec<Py<PyAny>>) -> PyResult<()> {
        let callbacks = PyObjects::from_vec(callbacks);
        let old = self.write(py, |n| std::mem::replace(&mut n.data.on_edge_add_callbacks, callbacks))?;
        drop(old);
        Ok(())
    }

    /// The owning Vertex's `on_node_update_callbacks` (a live list).
    #[getter]
    fn on_update_callbacks(&self, py: Python<'_>) -> PyResult<Py<PyList>> {
        Ok(self.vertex.try_borrow(py)?.on_node_update_callbacks.clone_ref(py))
    }

    /// The Vertex this node belongs to.
    #[getter]
    fn vertex(&self, py: Python<'_>) -> Py<Vertex> {
        self.vertex.clone_ref(py)
    }

    /// Depth-first traversal (exposed as `_traverse`, wrapped as `traverse`
    /// in `ironweaver/__init__.py`).
    #[pyo3(name = "_traverse", signature = (depth=None, filter=None, edge_filter=None))]
    fn traverse_nodes(
        &self,
        py: Python<'_>,
        depth: Option<usize>,
        filter: Option<AttrMap>,
        edge_filter: Option<Py<PyAny>>,
    ) -> PyResult<Py<Vertex>> {
        self.walk(py, depth, filter, edge_filter, false)
    }

    /// Every path from this node with `min_hops..=max_hops` edges, as `Path`
    /// objects (`path.nodes`, `path.edges`), depth first.
    ///
    /// `direction`: "out" (default), "in" or "both". `types`: edge type(s)
    /// to follow; `where`: an Expr every edge must match. `uniqueness`:
    /// "trail" (default: no edge twice), "path" (no node twice) or "walk"
    /// (anything; needs `max_hops`). `limit` stops after that many paths.
    #[pyo3(signature = (
        min_hops=1,
        max_hops=None,
        *,
        direction=None,
        types=None,
        r#where=None,
        uniqueness="trail",
        limit=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn paths<'py>(
        &self,
        py: Python<'py>,
        min_hops: usize,
        max_hops: Option<usize>,
        direction: Option<&'py str>,
        types: Option<Bound<'py, PyAny>>,
        r#where: Option<Bound<'py, PyAny>>,
        uniqueness: &'py str,
        limit: Option<usize>,
    ) -> PyResult<Py<pyo3::types::PyList>> {
        self.read(py, |_| ())?;
        let opts =
            crate::vertex::query::PathOptions { min_hops, max_hops, direction, types, r#where, uniqueness, limit };
        crate::vertex::query::node_paths(py, &self.vertex, self.ix, opts)
    }

    /// Breadth-first traversal of reachable nodes. Returns a new Vertex with
    /// copies of the reached nodes (and the edges between them); the visiting
    /// order is in `meta["nodelist"]`.
    #[pyo3(signature = (depth=None, filter=None, edge_filter=None))]
    fn bfs(
        &self,
        py: Python<'_>,
        depth: Option<usize>,
        filter: Option<AttrMap>,
        edge_filter: Option<Py<PyAny>>,
    ) -> PyResult<Py<Vertex>> {
        self.walk(py, depth, filter, edge_filter, true)
    }

    /// Search for the node `target_id` (bidirectional BFS). Returns the
    /// node if it is reachable within `depth` edges, None otherwise.
    #[pyo3(signature = (target_id, depth=None, filter=None, edge_filter=None))]
    fn bfs_search(
        &self,
        py: Python<'_>,
        target_id: String,
        depth: Option<usize>,
        filter: Option<AttrMap>,
        edge_filter: Option<Py<PyAny>>,
    ) -> PyResult<Option<Py<Node>>> {
        let v = self.vertex.try_borrow(py)?;
        v.graph.node(self.ix).ok_or_else(stale)?;
        let target = match v.graph.node_ix(&target_id) {
            Some(t) => t,
            None => return Ok(None),
        };
        let ok = edge_predicate(py, &self.vertex, &filter, &edge_filter);
        let path = interrupt::polling(py, || {
            traversal::bidirectional_bfs(&v.graph, self.ix, target, depth, Direction::Out, ok)
        })??;
        path.map(|_| Node::handle(py, &self.vertex, target)).transpose()
    }

    /// Retrieve a value from ``attr`` by key ("labels" gives the labels).
    /// Returns ``None`` if the key does not exist.
    fn attr_get(&self, py: Python<'_>, key: &str) -> PyResult<Option<Py<PyAny>>> {
        let v = self.vertex.try_borrow(py)?;
        v.graph.node(self.ix).ok_or_else(stale)?;
        Ok(node_value(py, &v.graph, self.ix, key)?.map(Bound::unbind))
    }

    /// Set a value in ``attr`` under ``key``.
    /// Fires the vertex's ``on_node_update_callbacks`` if the value changed.
    fn attr_set(slf: &Bound<'_, Self>, key: String, value: Py<PyAny>) -> PyResult<()> {
        let py = slf.py();
        let this = slf.get();
        let old = {
            let v = this.vertex.try_borrow(py)?;
            v.graph.node(this.ix).ok_or_else(stale)?;
            node_value(py, &v.graph, this.ix, &key)?
        };
        let changed = match &old {
            Some(o) => !o.rich_compare(value.bind(py), CompareOp::Eq)?.is_truthy()?,
            None => true,
        };
        if key == "labels" {
            let labels: Vec<String> = value
                .extract(py)
                .map_err(|_| pyo3::exceptions::PyTypeError::new_err("a node's \"labels\" must be a list of str"))?;
            this.set_labels(py, labels)?;
        } else {
            // Fetch the (unshared) dict only now: the comparison above may
            // have run Python code that derived a graph sharing it.
            this.attr_dict(py)?.bind(py).set_item(&key, &value)?;
        }
        let old = old.map(Bound::unbind);

        if changed {
            let callbacks = this.vertex.try_borrow(py)?.on_node_update_callbacks.clone_ref(py);
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

    /// Append ``value`` to a list stored at ``key`` in ``attr``.
    /// If the list does not exist, it will be created.
    #[pyo3(signature = (key, value))]
    fn attr_list_append(&self, py: Python<'_>, key: &str, value: Py<PyAny>) -> PyResult<()> {
        let dict = self.attr_dict(py)?;
        let dict = dict.bind(py);
        if let Some(existing) = dict.get_item(key)? {
            existing.cast::<PyList>()?.append(value)?;
            return Ok(());
        }
        dict.set_item(key, PyList::new(py, [value])?)
    }
}
