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
