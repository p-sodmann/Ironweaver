// vertex/algorithms/filter.rs

use pyo3::prelude::*;
use std::collections::HashSet;
use super::super::core::Vertex;
use super::super::subgraph::build_subgraph;

pub fn filter(
    vertex: &Vertex,
    py: Python<'_>,
    node_ids: Vec<String>
) -> PyResult<Py<Vertex>> {
    // Convert node_ids to a HashSet for efficient lookups
    let filter_set: HashSet<String> = node_ids.into_iter().collect();

    // Validate that all requested nodes exist in the source vertex
    for node_id in &filter_set {
        if !vertex.nodes.contains_key(node_id) {
            return Err(pyo3::exceptions::PyValueError::new_err(
                format!("Node with id '{}' not found in vertex", node_id)
            ));
        }
    }

    // The filtered view shares meta and callbacks with its source vertex.
    build_subgraph(py, vertex, &filter_set, vertex.meta.clone_ref(py), true)
}
