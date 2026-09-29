// vertex/subgraph.rs
//
// Building a new Vertex from part of another one (filter, expand, shortest
// paths, traversals).

use ironweaver_core::NodeIx;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use super::Vertex;

/// A new Vertex with copies of the nodes `keep` of `source` (in that order)
/// and of every edge between two of them. Node and edge `attr` / `meta`
/// dicts are shallow-copied (values are shared).
///
/// If `share_callbacks` is true the new vertex reuses the callback lists of
/// `source`; otherwise it gets fresh, empty lists. `meta` becomes the new
/// vertex's `meta` dict.
pub fn build_subgraph(
    py: Python<'_>,
    source: &Vertex,
    keep: impl IntoIterator<Item = NodeIx>,
    meta: Py<PyDict>,
    share_callbacks: bool,
) -> PyResult<Py<Vertex>> {
    let graph = source.graph.induced_subgraph(keep, |n| n.data.copy(py), |e| e.data.copy(py))?;
    let mut vertex = Vertex::with_graph(py, graph);
    vertex.meta = meta;
    if share_callbacks {
        vertex.on_node_add_callbacks = source.on_node_add_callbacks.clone_ref(py);
        vertex.on_edge_add_callbacks = source.on_edge_add_callbacks.clone_ref(py);
        vertex.on_node_update_callbacks = source.on_node_update_callbacks.clone_ref(py);
        vertex.on_edge_update_callbacks = source.on_edge_update_callbacks.clone_ref(py);
    }
    Py::new(py, vertex)
}
