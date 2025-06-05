// vertex/algorithms/filter.rs

use pyo3::prelude::*;
use std::collections::HashMap;
use crate::{Node, Edge};
use super::super::core::Vertex;

pub fn filter(
    vertex: &Vertex,
    py: Python<'_>,
    node_ids: Vec<String>
) -> PyResult<Py<Vertex>> {
    use std::collections::HashSet;
    
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
    
    // First pass: Create nodes with their original edges (we'll filter edges in second pass)
    let mut result_nodes = HashMap::<String, Py<Node>>::new();
    
    for node_id in &filter_set {
        if let Some(source_node) = vertex.nodes.get(node_id) {
            let source_node_ref = source_node.bind(py);

            // Get node attributes
            let attr: HashMap<String, Py<PyAny>> = source_node_ref.getattr("attr")?.extract().unwrap_or_default();

            // Get all edges from the source node
            let source_edges: Vec<Py<Edge>> = source_node_ref.getattr("edges")?.extract().unwrap_or_default();
            
            // Filter edges to only include those pointing to nodes that are also in our filter set
            let mut filtered_edges = Vec::new();
            for edge in source_edges {
                let edge_ref = edge.bind(py);
                let to_node: Py<Node> = edge_ref.getattr("to_node")?.extract()?;
                let to_node_ref = to_node.bind(py);
                let to_id = to_node_ref.getattr("id")?.extract::<String>()?;
                
                // Only include edge if target is also in the filter set
                if filter_set.contains(&to_id) {
                    // Keep the original edge but we'll need to update the node references
                    // after all nodes are created
                    filtered_edges.push(edge.clone_ref(py));
                }
            }
            
            // Create new node with filtered edges
            let new_node = Py::new(py, Node::new(node_id.clone(), Some(attr), Some(filtered_edges)))?;
            result_nodes.insert(node_id.clone(), new_node);
        }
    }
    
    // Second pass: Update edge references to point to the new nodes in our result set
    let mut final_result_nodes = HashMap::<String, Py<Node>>::new();
    
    for (node_id, node) in &result_nodes {
        let node_ref = node.bind(py);
        let attr: HashMap<String, Py<PyAny>> = node_ref.getattr("attr")?.extract().unwrap_or_default();
        let edges: Vec<Py<Edge>> = node_ref.getattr("edges")?.extract().unwrap_or_default();
        
        // Create new edges with proper node references from our result set
        let mut updated_edges = Vec::new();
        for edge in edges {
            let edge_ref = edge.bind(py);
            let to_node: Py<Node> = edge_ref.getattr("to_node")?.extract()?;
            let to_node_ref = to_node.bind(py);
            let to_id = to_node_ref.getattr("id")?.extract::<String>()?;
            
            // Get the target node from our result set
            if let Some(target_node) = result_nodes.get(&to_id) {
                let edge_attr: HashMap<String, Py<PyAny>> = edge_ref.getattr("attr")?.extract().unwrap_or_default();
                let edge_id: Option<String> = edge_ref.getattr("id").ok().and_then(|id| id.extract().ok());
                
                let new_edge = Py::new(py, Edge::new(
                    node.clone_ref(py),
                    target_node.clone_ref(py),
                    Some(edge_attr),
                    edge_id
                ))?;
                updated_edges.push(new_edge);
            }
        }
        
        // Create final node with updated edges
        let final_node = Py::new(py, Node::new(node_id.clone(), Some(attr), Some(updated_edges)))?;
        final_result_nodes.insert(node_id.clone(), final_node);
    }
    
    let result_vertex = Vertex::from_nodes(py, final_result_nodes);
    Py::new(py, result_vertex)
}
