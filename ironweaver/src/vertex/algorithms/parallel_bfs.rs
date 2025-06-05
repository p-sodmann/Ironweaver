use pyo3::prelude::*;
use std::collections::{HashMap, HashSet};
use crate::{Node, Edge};
use super::super::core::Vertex;
use rayon::prelude::*;

pub fn parallel_bfs(
    vertex: &Vertex,
    py: Python<'_>,
    start_node_id: String,
    target_node_id: String,
    max_depth: Option<usize>
) -> PyResult<Py<Vertex>> {
    // Ensure start and target exist
    if !vertex.nodes.contains_key(&start_node_id) {
        return Err(pyo3::exceptions::PyValueError::new_err(
            format!("Start node with id '{}' not found", start_node_id)
        ));
    }
    if !vertex.nodes.contains_key(&target_node_id) {
        return Err(pyo3::exceptions::PyValueError::new_err(
            format!("Target node with id '{}' not found", target_node_id)
        ));
    }

    // If start is target, return single-node vertex
    if start_node_id == target_node_id {
        if let Some(node) = vertex.nodes.get(&start_node_id) {
            let mut map = HashMap::new();
            map.insert(start_node_id.clone(), node.clone_ref(py));
            return Py::new(py, Vertex::from_nodes(py, map));
        }
    }

    let mut visited = HashSet::<String>::new();
    let mut parents = HashMap::<String, String>::new();
    let mut current_level = vec![start_node_id.clone()];
    visited.insert(start_node_id.clone());
    let mut depth = 0usize;

    while !current_level.is_empty() {
        if let Some(max_d) = max_depth {
            if depth >= max_d {
                break;
            }
        }

        // Collect neighbors of current level in parallel
        let neighbors: Vec<(String, String)> = current_level
            .par_iter()
            .flat_map(|node_id| {
                let node_id = node_id.clone();
                Python::with_gil(|gil| {
                    vertex.nodes.get(&node_id)
                        .map(|node| {
                            let node_ref = node.bind(gil);
                            let edges: Vec<Py<Edge>> = node_ref.getattr("edges")
                                .ok()
                                .and_then(|o| o.extract().ok())
                                .unwrap_or_default();
                            edges.into_iter().filter_map(|edge| {
                                let edge_ref = edge.bind(gil);
                                let to_node: Option<Py<Node>> = edge_ref.getattr("to_node").ok().and_then(|o| o.extract().ok());
                                to_node.and_then(|n| {
                                    let r = n.bind(gil);
                                    r.getattr("id").ok().and_then(|o| o.extract::<String>().ok()).map(|id| (id, node_id.clone()))
                                })
                            }).collect::<Vec<(String, String)>>()
                        })
                        .unwrap_or_default()
                })
            })
            .collect();

        let mut next_level = Vec::new();
        for (child, parent) in neighbors {
            if !visited.contains(&child) {
                visited.insert(child.clone());
                parents.insert(child.clone(), parent);
                if child == target_node_id {
                    // reconstruct path
                    let mut path_ids = Vec::new();
                    let mut current = child.clone();
                    path_ids.push(current.clone());
                    while let Some(p) = parents.get(&current) {
                        path_ids.push(p.clone());
                        current = p.clone();
                    }
                    let path_set: HashSet<String> = path_ids.iter().cloned().collect();
                    let mut path_nodes = HashMap::new();
                    for pid in &path_ids {
                        if let Some(orig) = vertex.nodes.get(pid) {
                            let orig_ref = orig.bind(py);
                            let attr: HashMap<String, Py<PyAny>> = orig_ref.getattr("attr").ok().and_then(|o| o.extract().ok()).unwrap_or_default();
                            let orig_edges: Vec<Py<Edge>> = orig_ref.getattr("edges").ok().and_then(|o| o.extract().ok()).unwrap_or_default();
                            let mut filtered = Vec::new();
                            for e in orig_edges {
                                let e_ref = e.bind(py);
                                if let Ok(to_node) = e_ref.getattr("to_node") {
                                    if let Ok(to_py) = to_node.extract::<Py<Node>>() {
                                        let to_ref = to_py.bind(py);
                                        if let Ok(tid) = to_ref.getattr("id") {
                                            if let Ok(tid_str) = tid.extract::<String>() {
                                                if path_set.contains(&tid_str) {
                                                    filtered.push(e.clone_ref(py));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            let new_node = Py::new(py, Node::new(pid.clone(), Some(attr), Some(filtered)))?;
                            path_nodes.insert(pid.clone(), new_node);
                        }
                    }
                    let result_vertex = Vertex::from_nodes(py, path_nodes);
                    return Py::new(py, result_vertex);
                }
                next_level.push(child);
            }
        }
        current_level = next_level;
        depth += 1;
    }

    Err(pyo3::exceptions::PyValueError::new_err(
        format!("Target node '{}' not reachable from '{}' within max_depth {:?}",
                target_node_id, start_node_id, max_depth)
    ))
}
