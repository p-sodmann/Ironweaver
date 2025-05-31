// vertex/algorithms/random_walks.rs

use pyo3::prelude::*;
use pyo3::types::PyList;
use std::collections::HashSet;
use crate::{Node, Edge};
use super::super::core::Vertex;
use rand::seq::SliceRandom;
use rand::thread_rng;

pub fn random_walks(
    vertex: &Vertex,
    py: Python<'_>,
    start_node_id: String,
    max_length: usize,
    min_length: Option<usize>,
    num_attempts: usize,
    allow_revisit: Option<bool>
) -> PyResult<Py<PyList>> {
    // Validate start node exists
    if !vertex.nodes.contains_key(&start_node_id) {
        return Err(pyo3::exceptions::PyValueError::new_err(
            format!("Start node with id '{}' not found", start_node_id)
        ));
    }

    let min_len = min_length.unwrap_or(1);
    let allow_revisit_nodes = allow_revisit.unwrap_or(false);
    
    // Validate parameters
    if max_length == 0 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "max_length must be greater than 0"
        ));
    }
    
    if min_len > max_length {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "min_length cannot be greater than max_length"
        ));
    }

    let mut all_walks = Vec::new();
    let mut rng = thread_rng();

    // Perform multiple random walk attempts
    for _ in 0..num_attempts {
        if let Some(walk) = perform_simple_random_walk(
            vertex,
            py,
            start_node_id.clone(),
            max_length,
            allow_revisit_nodes,
            &mut rng
        )? {
            // Only add walks that meet minimum length requirement
            if walk.len() >= min_len {
                all_walks.push(walk);
            }
        }
    }

    // Remove duplicates
    let mut unique_walks = Vec::new();
    let mut seen_walks = HashSet::new();
    
    for walk in all_walks {
        let walk_key = walk.join(","); // Create a string representation for comparison
        if !seen_walks.contains(&walk_key) {
            seen_walks.insert(walk_key);
            unique_walks.push(walk);
        }
    }

    // Convert to Python list of lists
    let result = PyList::empty(py);
    for walk in unique_walks {
        let py_walk = PyList::empty(py);
        for node_id in walk {
            py_walk.append(node_id)?;
        }
        result.append(py_walk)?;
    }
    
    Ok(result.into())
}

// Simple random walk function that embraces randomness without backtracking
fn perform_simple_random_walk(
    vertex: &Vertex,
    py: Python<'_>,
    start_node_id: String,
    max_length: usize,
    allow_revisit: bool,
    rng: &mut rand::rngs::ThreadRng
) -> PyResult<Option<Vec<String>>> {
    let mut walk = Vec::new();
    let mut visited = HashSet::new();
    let mut current_node_id = start_node_id;

    // Start the walk
    for _ in 0..max_length {
        // Add current node to walk
        walk.push(current_node_id.clone());
        
        // Track visited nodes if revisiting is not allowed
        if !allow_revisit {
            visited.insert(current_node_id.clone());
        }

        // Get the current node
        let current_node = match vertex.nodes.get(&current_node_id) {
            Some(node) => node,
            None => break, // Node not found, end walk
        };

        // Get edges from current node
        let node_ref = current_node.bind(py);
        let edges: Vec<Py<Edge>> = match node_ref.getattr("edges") {
            Ok(edges_attr) => match edges_attr.extract() {
                Ok(edges) => edges,
                Err(_) => break, // No edges or extraction failed, end walk
            },
            Err(_) => break, // No edges attribute, end walk
        };

        // Collect valid next nodes
        let mut valid_next_nodes = Vec::new();
        for edge in edges {
            let edge_ref = edge.bind(py);
            if let Ok(to_node) = edge_ref.getattr("to_node") {
                if let Ok(to_node_py) = to_node.extract::<Py<Node>>() {
                    let to_node_ref = to_node_py.bind(py);
                    if let Ok(to_id) = to_node_ref.getattr("id") {
                        if let Ok(to_id_str) = to_id.extract::<String>() {
                            // Include node if revisiting is allowed OR if we haven't visited it
                            if allow_revisit || !visited.contains(&to_id_str) {
                                valid_next_nodes.push(to_id_str);
                            }
                        }
                    }
                }
            }
        }

        // If no valid next nodes, end the walk
        if valid_next_nodes.is_empty() {
            break;
        }

        // Randomly choose next node - this is where the true randomness happens
        if let Some(next_node) = valid_next_nodes.choose(rng) {
            current_node_id = next_node.clone();
        } else {
            break; // Should not happen, but handle gracefully
        }
    }

    Ok(Some(walk))
}
