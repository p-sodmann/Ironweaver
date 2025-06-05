// vertex/algorithms/random_walks.rs

use pyo3::prelude::*;
use pyo3::types::PyList;
use std::collections::{HashMap, HashSet};
use crate::{Node, Edge};
use super::super::core::Vertex;
use rand::seq::SliceRandom;
use rand::thread_rng;

#[derive(Clone)]
struct Walk {
    nodes: Vec<String>,
    edges: Vec<String>,
}

pub fn random_walks(
    vertex: &Vertex,
    py: Python<'_>,
    start_node_id: String,
    max_length: usize,
    min_length: Option<usize>,
    num_attempts: usize,
    allow_revisit: Option<bool>,
    include_edge_types: Option<bool>,
    edge_type_field: Option<String>
) -> PyResult<Py<PyList>> {
    if !vertex.nodes.contains_key(&start_node_id) {
        return Err(pyo3::exceptions::PyValueError::new_err(
            format!("Start node with id '{}' not found", start_node_id)
        ));
    }

    let min_len = min_length.unwrap_or(1);
    let allow_revisit_nodes = allow_revisit.unwrap_or(false);
    let include_edges = include_edge_types.unwrap_or(false);
    let type_field = edge_type_field.unwrap_or_else(|| "type".to_string());

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

    // Build adjacency list with optional edge types
    let mut adjacency: HashMap<String, Vec<(String, Option<String>)>> = HashMap::new();
    for (id, node) in &vertex.nodes {
        let node_ref = node.bind(py);
        let edges: Vec<Py<Edge>> = node_ref.getattr("edges")?.extract().unwrap_or_default();
        let mut neigh = Vec::new();
        for edge in edges {
            let edge_ref = edge.bind(py);
            let to_node: Py<Node> = edge_ref.getattr("to_node")?.extract()?;
            let to_id = to_node.bind(py).getattr("id")?.extract::<String>()?;
            let e_type = if include_edges {
                edge_ref
                    .getattr("attr")
                    .ok()
                    .and_then(|attr| attr.get_item(&type_field).ok())
                    .and_then(|val| val.extract::<String>().ok())
            } else { None };
            neigh.push((to_id, e_type));
        }
        adjacency.insert(id.clone(), neigh);
    }

    let mut all_walks = Vec::new();
    let mut rng = thread_rng();
    for _ in 0..num_attempts {
        if let Some(walk) = perform_walk(&adjacency, &start_node_id, max_length, allow_revisit_nodes, include_edges, &mut rng) {
            if walk.nodes.len() >= min_len {
                all_walks.push(walk);
            }
        }
    }

    let mut unique_walks = Vec::new();
    let mut seen = HashSet::new();
    for walk in all_walks {
        let key = if include_edges {
            format!("{}|{}", walk.nodes.join(","), walk.edges.join(","))
        } else {
            walk.nodes.join(",")
        };
        if seen.insert(key) {
            unique_walks.push(walk);
        }
    }

    let result = PyList::empty(py);
    for walk in unique_walks {
        if include_edges {
            let py_walk = PyList::empty(py);
            for i in 0..walk.nodes.len() {
                py_walk.append(&walk.nodes[i])?;
                if i < walk.edges.len() {
                    py_walk.append(&walk.edges[i])?;
                }
            }
            result.append(py_walk)?;
        } else {
            let py_walk = PyList::empty(py);
            for id in walk.nodes {
                py_walk.append(id)?;
            }
            result.append(py_walk)?;
        }
    }

    Ok(result.into())
}

fn perform_walk(
    adjacency: &HashMap<String, Vec<(String, Option<String>)>>,
    start: &str,
    max_length: usize,
    allow_revisit: bool,
    include_edge_types: bool,
    rng: &mut rand::rngs::ThreadRng
) -> Option<Walk> {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut visited = HashSet::new();
    let mut current = start.to_string();

    for _ in 0..max_length {
        nodes.push(current.clone());
        if !allow_revisit {
            visited.insert(current.clone());
        }
        let neighbors = adjacency.get(&current)?;
        let mut options = Vec::new();
        for (to_id, edge_type) in neighbors {
            if allow_revisit || !visited.contains(to_id) {
                options.push((to_id.clone(), edge_type.clone()));
            }
        }
        if options.is_empty() { break; }
        let (next, e_type) = options.choose(rng)?.clone();
        if include_edge_types {
            edges.push(e_type.unwrap_or_else(|| "unknown".to_string()));
        }
        current = next;
    }

    Some(Walk { nodes, edges })
}

