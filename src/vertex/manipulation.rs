// vertex/manipulation.rs

use pyo3::prelude::*;
use std::collections::HashMap;
use crate::{Node, Edge};
use super::Vertex;

pub fn add_node(
    vertex: &mut Vertex,
    py: Python<'_>, 
    id: String, 
    attr: Option<HashMap<String, Py<PyAny>>>
) -> PyResult<Py<Node>> {
    // Check if node already exists
    if vertex.nodes.contains_key(&id) {
        return Err(pyo3::exceptions::PyValueError::new_err(
            format!("Node with id '{}' already exists", id)
        ));
    }

    // Create new node
    let node = Py::new(py, Node::new(py, id.clone(), attr, None))?;
    
    // Add to nodes hashmap
    vertex.nodes.insert(id, node.clone_ref(py));
    
    Ok(node)
}

pub fn add_edge(
    vertex: &mut Vertex,
    py: Python<'_>,
    from_id: String,
    to_id: String,
    attr: Option<HashMap<String, Py<PyAny>>>
) -> PyResult<Py<Edge>> {
    // Get the from and to nodes
    let from_node = vertex.nodes.get(&from_id)
        .ok_or_else(|| pyo3::exceptions::PyValueError::new_err(
            format!("Node with id '{}' not found", from_id)
        ))?
        .clone_ref(py);
        
    let to_node = vertex.nodes.get(&to_id)
        .ok_or_else(|| pyo3::exceptions::PyValueError::new_err(
            format!("Node with id '{}' not found", to_id)
        ))?
        .clone_ref(py);

    // Create the edge
    let edge = Py::new(py, Edge::new(py, from_node.clone_ref(py), to_node.clone_ref(py), attr, None))?;

    // Add the edge to the from_node's edges list
    let mut from_node_ref = from_node.borrow_mut(py);
    from_node_ref.edges.push(edge.clone_ref(py));
    drop(from_node_ref); // Release the borrow before borrowing to_node
    
    // Add the edge to the to_node's inverse_edges list
    let mut to_node_ref = to_node.borrow_mut(py);
    to_node_ref.inverse_edges.push(edge.clone_ref(py));

    Ok(edge)
}

pub fn get_node(vertex: &Vertex, py: Python<'_>, id: String) -> PyResult<Py<Node>> {
    vertex.nodes
        .get(&id)
        .map(|n| n.clone_ref(py))
        .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err(
            format!("Node with id '{}' not found", id)
        ))
}

/// Remove edges and inverse_edges that point to nodes not present in the vertex.
/// Returns the number of edges removed.
pub fn prune(vertex: &Vertex, py: Python<'_>) -> PyResult<usize> {
    let mut removed = 0usize;

    for node_py in vertex.nodes.values() {
        let mut node_ref = node_py.bind(py).borrow_mut();

        let before_edges = node_ref.edges.len();
        node_ref.edges.retain(|edge| {
            let edge_ref = edge.bind(py);
            let to_id = edge_ref
                .borrow()
                .to_node
                .bind(py)
                .borrow()
                .id
                .clone();
            vertex.nodes.contains_key(&to_id)
        });
        removed += before_edges - node_ref.edges.len();

        let before_inv = node_ref.inverse_edges.len();
        node_ref.inverse_edges.retain(|edge| {
            let edge_ref = edge.bind(py);
            let from_id = edge_ref
                .borrow()
                .from_node
                .bind(py)
                .borrow()
                .id
                .clone();
            vertex.nodes.contains_key(&from_id)
        });
        removed += before_inv - node_ref.inverse_edges.len();
    }

    Ok(removed)
}

/// Remove a node and every edge incident to it. Neighbours' `edges` /
/// `inverse_edges` lists are updated so that no dangling edges remain.
/// Returns the removed node, detached from the graph.
pub fn remove_node(vertex: &mut Vertex, py: Python<'_>, id: &str) -> PyResult<Py<Node>> {
    let node = vertex.nodes.remove(id).ok_or_else(|| {
        pyo3::exceptions::PyKeyError::new_err(format!("Node with id '{}' not found", id))
    })?;

    let (out_edges, in_edges) = {
        let mut n = node.borrow_mut(py);
        n.vertex = None;
        (std::mem::take(&mut n.edges), std::mem::take(&mut n.inverse_edges))
    };
    for edge in &out_edges {
        let target = edge.borrow(py).to_node.clone_ref(py);
        if !target.is(&node) {
            target.borrow_mut(py).inverse_edges.retain(|e| !e.is(edge));
        }
    }
    for edge in &in_edges {
        let source = edge.borrow(py).from_node.clone_ref(py);
        if !source.is(&node) {
            source.borrow_mut(py).edges.retain(|e| !e.is(edge));
        }
    }
    Ok(node)
}

/// Remove edges from `from_id` to `to_id`. If `attr` is given, only edges
/// whose attributes equal every given key/value pair are removed.
/// Returns the number of edges removed.
pub fn remove_edge(
    vertex: &Vertex,
    py: Python<'_>,
    from_id: &str,
    to_id: &str,
    attr: Option<HashMap<String, Py<PyAny>>>,
) -> PyResult<usize> {
    let lookup = |id: &str| {
        vertex.nodes.get(id).map(|n| n.clone_ref(py)).ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err(format!("Node with id '{}' not found", id))
        })
    };
    let from_node = lookup(from_id)?;
    let to_node = lookup(to_id)?;

    let candidates: Vec<Py<Edge>> = from_node.borrow(py).edges.iter().map(|e| e.clone_ref(py)).collect();
    let mut doomed: Vec<Py<Edge>> = Vec::new();
    'edges: for edge in candidates {
        if !edge.borrow(py).to_node.is(&to_node) {
            continue;
        }
        if let Some(filter) = &attr {
            for (key, expected) in filter {
                let value = edge.borrow(py).attr.get(key).map(|v| v.clone_ref(py));
                match value {
                    Some(v) if v.bind(py).eq(expected.bind(py))? => {}
                    _ => continue 'edges,
                }
            }
        }
        doomed.push(edge);
    }

    if !doomed.is_empty() {
        let is_doomed = |e: &Py<Edge>| doomed.iter().any(|d| d.is(e));
        from_node.borrow_mut(py).edges.retain(|e| !is_doomed(e));
        to_node.borrow_mut(py).inverse_edges.retain(|e| !is_doomed(e));
    }
    Ok(doomed.len())
}
