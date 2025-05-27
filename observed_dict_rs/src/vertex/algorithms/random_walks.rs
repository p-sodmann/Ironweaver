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
    num_attempts: usize
) -> PyResult<Py<PyList>> {
    // Validate start node exists
    if !vertex.nodes.contains_key(&start_node_id) {
        return Err(pyo3::exceptions::PyValueError::new_err(
            format!("Start node with id '{}' not found", start_node_id)
        ));
    }

    let min_len = min_length.unwrap_or(1);
    
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
        let mut current_walk = Vec::new();
        let mut visited = HashSet::new();
        
        // Try to perform a random walk with backtracking
        if let Some(walk) = perform_random_walk_with_min_length(
            vertex,
            py,
            start_node_id.clone(),
            max_length,
            min_len,
            &mut visited,
            &mut current_walk,
            &mut rng
        )? {
            // Walk already meets minimum length requirement due to our algorithm
            all_walks.push(walk);
        }
        // If perform_random_walk returns None, it means no valid walk was found
        // This can happen in graphs where no path from start_node can reach min_length
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

// Recursive helper function to perform a single random walk with minimum length consideration
fn perform_random_walk_with_min_length(
    vertex: &Vertex,
    py: Python<'_>,
    current_node_id: String,
    remaining_length: usize,
    min_length: usize,
    visited: &mut HashSet<String>,
    current_walk: &mut Vec<String>,
    rng: &mut rand::rngs::ThreadRng
) -> PyResult<Option<Vec<String>>> {
    // Add current node to the walk
    current_walk.push(current_node_id.clone());
    visited.insert(current_node_id.clone());

    // If we've reached maximum length
    if remaining_length == 0 || current_walk.len() >= 1000 { // Safety limit to prevent infinite recursion
        let result = if current_walk.len() >= min_length {
            Some(current_walk.clone())
        } else {
            None // Walk doesn't meet minimum length
        };
        // Backtrack: remove current node from visited and walk for other attempts
        current_walk.pop();
        visited.remove(&current_node_id);
        return Ok(result);
    }

    // Check if we can still potentially reach minimum length
    // If current walk length + remaining length < min_length, we can't succeed
    if current_walk.len() + remaining_length < min_length {
        // Can't possibly reach minimum length, backtrack early
        current_walk.pop();
        visited.remove(&current_node_id);
        return Ok(None);
    }

    // Get the current node
    let current_node = match vertex.nodes.get(&current_node_id) {
        Some(node) => node,
        None => {
            // Node not found, backtrack
            current_walk.pop();
            visited.remove(&current_node_id);
            return Ok(None);
        }
    };

    // Get edges from current node
    let node_ref = current_node.bind(py);
    let edges: Vec<Py<Edge>> = match node_ref.getattr("edges") {
        Ok(edges_attr) => match edges_attr.extract() {
            Ok(edges) => edges,
            Err(_) => {
                // No edges or extraction failed, check if current walk meets minimum length
                let result = if current_walk.len() >= min_length {
                    Some(current_walk.clone())
                } else {
                    None
                };
                current_walk.pop();
                visited.remove(&current_node_id);
                return Ok(result);
            }
        },
        Err(_) => {
            // No edges attribute, check if current walk meets minimum length
            let result = if current_walk.len() >= min_length {
                Some(current_walk.clone())
            } else {
                None
            };
            current_walk.pop();
            visited.remove(&current_node_id);
            return Ok(result);
        }
    };

    // Filter out edges that lead to already visited nodes
    let mut valid_edges = Vec::new();
    for edge in edges {
        let edge_ref = edge.bind(py);
        if let Ok(to_node) = edge_ref.getattr("to_node") {
            if let Ok(to_node_py) = to_node.extract::<Py<Node>>() {
                let to_node_ref = to_node_py.bind(py);
                if let Ok(to_id) = to_node_ref.getattr("id") {
                    if let Ok(to_id_str) = to_id.extract::<String>() {
                        if !visited.contains(&to_id_str) {
                            valid_edges.push((edge, to_id_str));
                        }
                    }
                }
            }
        }
    }

    // If no valid edges, this is a dead end - check if we meet minimum length
    if valid_edges.is_empty() {
        let result = if current_walk.len() >= min_length {
            Some(current_walk.clone())
        } else {
            None
        };
        current_walk.pop();
        visited.remove(&current_node_id);
        return Ok(result);
    }

    // Shuffle the valid edges to try them in random order for backtracking
    valid_edges.shuffle(rng);

    // Try each valid edge - backtrack if none lead to a successful walk
    for (_, next_node_id) in valid_edges {
        // Try this path recursively
        if let Ok(Some(result_walk)) = perform_random_walk_with_min_length(
            vertex,
            py,
            next_node_id.clone(),
            remaining_length - 1,
            min_length,
            visited,
            current_walk,
            rng
        ) {
            // Found a successful walk, clean up and return it
            current_walk.pop();
            visited.remove(&current_node_id);
            return Ok(Some(result_walk));
        }
        // If this path didn't work, the recursive call already cleaned up,
        // so we continue to try the next edge
    }

    // None of the edges led to a successful walk that meets minimum length
    // Check if current walk meets minimum length as a fallback
    let result = if current_walk.len() >= min_length {
        Some(current_walk.clone())
    } else {
        None
    };
    current_walk.pop();
    visited.remove(&current_node_id);
    Ok(result)
}

// Recursive helper function to perform a single random walk with backtracking
fn perform_random_walk(
    vertex: &Vertex,
    py: Python<'_>,
    current_node_id: String,
    remaining_length: usize,
    visited: &mut HashSet<String>,
    current_walk: &mut Vec<String>,
    rng: &mut rand::rngs::ThreadRng
) -> PyResult<Option<Vec<String>>> {
    // Add current node to the walk
    current_walk.push(current_node_id.clone());
    visited.insert(current_node_id.clone());

    // If we've reached maximum length, return the walk
    if remaining_length == 0 || current_walk.len() >= 1000 { // Safety limit to prevent infinite recursion
        let result = current_walk.clone();
        // Backtrack: remove current node from visited and walk for other attempts
        current_walk.pop();
        visited.remove(&current_node_id);
        return Ok(Some(result));
    }

    // Get the current node
    let current_node = match vertex.nodes.get(&current_node_id) {
        Some(node) => node,
        None => {
            // Node not found, backtrack
            current_walk.pop();
            visited.remove(&current_node_id);
            return Ok(None);
        }
    };

    // Get edges from current node
    let node_ref = current_node.bind(py);
    let edges: Vec<Py<Edge>> = match node_ref.getattr("edges") {
        Ok(edges_attr) => match edges_attr.extract() {
            Ok(edges) => edges,
            Err(_) => {
                // No edges or extraction failed, backtrack
                current_walk.pop();
                visited.remove(&current_node_id);
                return Ok(None);
            }
        },
        Err(_) => {
            // No edges attribute, backtrack
            current_walk.pop();
            visited.remove(&current_node_id);
            return Ok(None);
        }
    };

    // Filter out edges that lead to already visited nodes
    let mut valid_edges = Vec::new();
    for edge in edges {
        let edge_ref = edge.bind(py);
        if let Ok(to_node) = edge_ref.getattr("to_node") {
            if let Ok(to_node_py) = to_node.extract::<Py<Node>>() {
                let to_node_ref = to_node_py.bind(py);
                if let Ok(to_id) = to_node_ref.getattr("id") {
                    if let Ok(to_id_str) = to_id.extract::<String>() {
                        if !visited.contains(&to_id_str) {
                            valid_edges.push((edge, to_id_str));
                        }
                    }
                }
            }
        }
    }

    // If no valid edges, this is a dead end - backtrack
    if valid_edges.is_empty() {
        current_walk.pop();
        visited.remove(&current_node_id);
        return Ok(None);
    }

    // Shuffle the valid edges to try them in random order for backtracking
    valid_edges.shuffle(rng);

    // Try each valid edge - backtrack if none lead to a successful walk
    for (_, next_node_id) in valid_edges {
        // Try this path recursively
        if let Ok(Some(result_walk)) = perform_random_walk(
            vertex,
            py,
            next_node_id.clone(),
            remaining_length - 1,
            visited,
            current_walk,
            rng
        ) {
            // Found a successful walk, clean up and return it
            current_walk.pop();
            visited.remove(&current_node_id);
            return Ok(Some(result_walk));
        }
        // If this path didn't work, the recursive call already cleaned up,
        // so we continue to try the next edge
    }

    // None of the edges led to a successful walk, backtrack
    current_walk.pop();
    visited.remove(&current_node_id);
    Ok(None)
}
