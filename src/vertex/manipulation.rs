// vertex/manipulation.rs

use pyo3::exceptions::{PyKeyError, PyValueError};
use pyo3::prelude::*;

use super::Vertex;
use crate::data::{AttrMap, PyGraph};
use crate::Node;

/// Remove a node and every edge attached to it. Returns a detached copy of
/// the node, in a new one-node Vertex.
pub fn remove_node(slf: &Bound<'_, Vertex>, id: &str) -> PyResult<Py<Node>> {
    let py = slf.py();
    let (id, data) = {
        let mut v = slf.try_borrow_mut()?;
        let ix = v.graph.node_ix(id).ok_or_else(|| PyKeyError::new_err(format!("Node with id '{}' not found", id)))?;
        v.graph.remove_node(ix).expect("looked up above")
    };
    let mut graph = PyGraph::new();
    let ix = graph.add_node(id, data).expect("empty graph");
    let detached = Py::new(py, Vertex::with_graph(py, graph))?;
    Node::handle(py, &detached, ix)
}

/// Remove edges from `from_id` to `to_id`. If `attr` is given, only edges
/// whose attributes equal every given key/value pair are removed ("type"
/// matches the edge's type).
/// Returns the number of edges removed.
pub fn remove_edge(slf: &Bound<'_, Vertex>, from_id: &str, to_id: &str, attr: Option<AttrMap>) -> PyResult<usize> {
    let py = slf.py();
    let doomed = {
        let v = slf.try_borrow()?;
        let lookup = |id: &str| {
            v.graph.node_ix(id).ok_or_else(|| PyValueError::new_err(format!("Node with id '{}' not found", id)))
        };
        let (from, to) = (lookup(from_id)?, lookup(to_id)?);
        let mut doomed = Vec::new();
        'edges: for &e in v.graph.node(from).expect("looked up above").out_edges() {
            let edge = v.graph.edge(e).expect("listed edges are live");
            if edge.target() != to {
                continue;
            }
            if let Some(filter) = &attr {
                for (key, expected) in filter {
                    match crate::data::edge_value(py, &v.graph, e, key)? {
                        Some(value) if value.eq(expected.bind(py))? => {}
                        _ => continue 'edges,
                    }
                }
            }
            doomed.push(e);
        }
        doomed
    };
    let removed: Vec<_> = {
        let mut v = slf.try_borrow_mut()?;
        doomed.iter().filter_map(|&e| v.graph.remove_edge(e)).collect()
    };
    Ok(removed.len())
}
