// vertex/algorithms/random_walks.rs

use pyo3::prelude::*;
use pyo3::types::PyList;
use std::collections::{HashMap, HashSet};
use crate::{Node, Edge};
use super::super::core::Vertex;
use rand::seq::SliceRandom;
use rand::thread_rng;
use rand::Rng;

// Structure to hold a walk with optional edge types
#[derive(Clone)]
struct Walk {
    nodes: Vec<String>,
    edges: Vec<String>, // Edge types between nodes
}

fn validate_params(
    vertex: &Vertex,
    start_node_id: &Option<String>,
    max_length: usize,
    min_len: usize,
    stratified: bool,
) -> PyResult<()> {
    match start_node_id {
        Some(id) => {
            if !vertex.nodes.contains_key(id) {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    format!("Start node with id '{}' not found", id),
                ));
            }
        }
        None => {
            if !stratified {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "start_node_id may only be None when stratified=True",
                ));
            }
            if vertex.nodes.is_empty() {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "Cannot perform stratified walks on an empty graph",
                ));
            }
        }
    }

    if max_length == 0 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "max_length must be greater than 0",
        ));
    }

    if min_len > max_length {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "min_length cannot be greater than max_length",
        ));
    }

    Ok(())
}

fn deduplicate_walks(walks: Vec<Walk>, include_edges: bool) -> Vec<Walk> {
    let mut unique_walks = Vec::new();
    let mut seen_walks = HashSet::new();

    for walk in walks {
        let walk_key = if include_edges {
            format!("{}|{}", walk.nodes.join(","), walk.edges.join(","))
        } else {
            walk.nodes.join(",")
        };

        if !seen_walks.contains(&walk_key) {
            seen_walks.insert(walk_key);
            unique_walks.push(walk);
        }
    }

    unique_walks
}

// Pick an index with probability proportional to its weight.
fn weighted_pick_index(weights: &[f64], rng: &mut rand::rngs::ThreadRng) -> usize {
    let total: f64 = weights.iter().sum();
    if total <= 0.0 {
        return 0;
    }
    let mut target = rng.gen::<f64>() * total;
    for (i, w) in weights.iter().enumerate() {
        target -= w;
        if target < 0.0 {
            return i;
        }
    }
    weights.len() - 1
}

// Weight used in stratified mode: inverse of how often the node was visited
// (smoothed so unvisited nodes have weight 1.0 instead of infinity).
fn stratified_weight(visit_counts: &HashMap<String, u64>, node_id: &str) -> f64 {
    1.0 / (1.0 + *visit_counts.get(node_id).unwrap_or(&0) as f64)
}

pub fn random_walks(
    vertex: &Vertex,
    py: Python<'_>,
    start_node_id: Option<String>,
    max_length: usize,
    min_length: Option<usize>,
    num_attempts: usize,
    allow_revisit: Option<bool>,
    include_edge_types: Option<bool>,
    edge_type_field: Option<String>,
    stratified: Option<bool>
) -> PyResult<Py<PyList>> {
    let min_len = min_length.unwrap_or(1);
    let allow_revisit_nodes = allow_revisit.unwrap_or(false);
    let include_edges = include_edge_types.unwrap_or(false);
    let type_field = edge_type_field.unwrap_or_else(|| "type".to_string());
    let stratified_mode = stratified.unwrap_or(false);

    validate_params(vertex, &start_node_id, max_length, min_len, stratified_mode)?;

    // Visit counts persist across all attempts of this call so that later
    // walks are steered towards nodes that earlier walks neglected.
    let mut visit_counts: HashMap<String, u64> = HashMap::new();

    let mut all_walks = Vec::new();
    let mut rng = thread_rng();    // Perform multiple random walk attempts
    for _ in 0..num_attempts {
        let walk_start = match &start_node_id {
            Some(id) => id.clone(),
            None => {
                // Stratified start: sample over all nodes, favouring the
                // least-visited ones.
                let ids: Vec<&String> = vertex.nodes.keys().collect();
                let weights: Vec<f64> = ids
                    .iter()
                    .map(|id| stratified_weight(&visit_counts, id))
                    .collect();
                ids[weighted_pick_index(&weights, &mut rng)].clone()
            }
        };

        if let Some(walk) = perform_simple_random_walk(
            vertex,
            py,
            walk_start,
            max_length,
            allow_revisit_nodes,
            include_edges,
            &type_field,
            stratified_mode,
            &mut visit_counts,
            &mut rng
        )? {
            // Only add walks that meet minimum length requirement
            if walk.nodes.len() >= min_len {
                all_walks.push(walk);
            }
        }
    }

    let unique_walks = deduplicate_walks(all_walks, include_edges);

    // Convert to Python list
    let result = PyList::empty(py);
    for walk in unique_walks {
        if include_edges {
            // Return list of [node, edge_type, node, edge_type, ...] format
            let py_walk = PyList::empty(py);
            for i in 0..walk.nodes.len() {
                py_walk.append(&walk.nodes[i])?;
                if i < walk.edges.len() {
                    py_walk.append(&walk.edges[i])?;
                }
            }
            result.append(py_walk)?;
        } else {
            // Return list of nodes only
            let py_walk = PyList::empty(py);
            for node_id in walk.nodes {
                py_walk.append(node_id)?;
            }
            result.append(py_walk)?;
        }
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
    include_edge_types: bool,
    edge_type_field: &str,
    stratified: bool,
    visit_counts: &mut HashMap<String, u64>,
    rng: &mut rand::rngs::ThreadRng
) -> PyResult<Option<Walk>> {    let mut walk_nodes = Vec::new();
    let mut walk_edges = Vec::new();
    let mut visited = HashSet::new();
    let mut current_node_id = start_node_id;

    // Start the walk
    for _ in 0..max_length {
        // Add current node to walk
        walk_nodes.push(current_node_id.clone());
        if stratified {
            *visit_counts.entry(current_node_id.clone()).or_insert(0) += 1;
        }

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

        // Collect valid next nodes and their corresponding edge types
        let mut valid_next_options = Vec::new();
        for edge in edges {
            let edge_ref = edge.bind(py);
            if let Ok(to_node) = edge_ref.getattr("to_node") {
                if let Ok(to_node_py) = to_node.extract::<Py<Node>>() {
                    let to_node_ref = to_node_py.bind(py);
                    if let Ok(to_id) = to_node_ref.getattr("id") {
                        if let Ok(to_id_str) = to_id.extract::<String>() {
                            // Include node if revisiting is allowed OR if we haven't visited it
                            if allow_revisit || !visited.contains(&to_id_str) {
                                // Get edge type if needed
                                let edge_type = if include_edge_types {
                                    edge_ref.getattr("attr")
                                        .ok()
                                        .and_then(|attr| attr.get_item(edge_type_field).ok())
                                        .and_then(|type_val| type_val.extract::<String>().ok())
                                        .unwrap_or_else(|| "unknown".to_string())
                                } else {
                                    String::new()
                                };
                                
                                valid_next_options.push((to_id_str, edge_type));
                            }
                        }
                    }
                }
            }
        }

        // If no valid next nodes, end the walk
        if valid_next_options.is_empty() {
            break;
        }

        // Randomly choose next option - this is where the true randomness happens.
        // In stratified mode the choice is biased towards the least-visited
        // candidates; otherwise it is uniform.
        let chosen = if stratified {
            let weights: Vec<f64> = valid_next_options
                .iter()
                .map(|(id, _)| stratified_weight(visit_counts, id))
                .collect();
            valid_next_options.get(weighted_pick_index(&weights, rng))
        } else {
            valid_next_options.choose(rng)
        };

        if let Some((next_node, edge_type)) = chosen {
            if include_edge_types {
                walk_edges.push(edge_type.clone());
            }
            current_node_id = next_node.clone();
        } else {
            break; // Should not happen, but handle gracefully
        }
    }

    Ok(Some(Walk {
        nodes: walk_nodes,
        edges: walk_edges,
    }))
}
