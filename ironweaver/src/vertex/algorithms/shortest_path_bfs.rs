// vertex/algorithms/shortest_path_bfs.rs

use pyo3::prelude::*;
use std::collections::{HashMap, HashSet, VecDeque};
use crate::{Node, Edge};
use super::super::core::Vertex;

pub fn shortest_path_bfs(
    vertex: &Vertex,
    py: Python<'_>,
    root_node_id: String,
    target_node_id: String,
    max_depth: Option<usize>
) -> PyResult<Py<Vertex>> {
    // Validate nodes exist
    if !vertex.nodes.contains_key(&root_node_id) {
        return Err(pyo3::exceptions::PyValueError::new_err(
            format!("Root node with id '{}' not found", root_node_id)
        ));
    }
    if !vertex.nodes.contains_key(&target_node_id) {
        return Err(pyo3::exceptions::PyValueError::new_err(
            format!("Target node with id '{}' not found", target_node_id)
        ));
    }

    // Special case: root == target
    if root_node_id == target_node_id {
        let mut path_nodes = HashMap::<String, Py<Node>>::new();
        if let Some(orig_node) = vertex.nodes.get(&root_node_id) {
            let orig_ref = orig_node.bind(py);
            let attr: HashMap<String, Py<PyAny>> = orig_ref.getattr("attr")?.extract().unwrap_or_default();
            let new_node = Py::new(py, Node::new(root_node_id.clone(), Some(attr), Some(Vec::new())))?;
            path_nodes.insert(root_node_id.clone(), new_node);
        }
        let result_vertex = Vertex::from_nodes(py, path_nodes);
        return Py::new(py, result_vertex);
    }

    // Build adjacency list once to avoid Python lookups during search
    let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();
    for (id, node) in &vertex.nodes {
        let node_ref = node.bind(py);
        let edges: Vec<Py<Edge>> = node_ref.getattr("edges")?.extract().unwrap_or_default();
        let mut neigh = Vec::new();
        for edge in edges {
            let edge_ref = edge.bind(py);
            let to_node: Py<Node> = edge_ref.getattr("to_node")?.extract()?;
            let to_id = to_node.bind(py).getattr("id")?.extract::<String>()?;
            neigh.push(to_id);
        }
        adjacency.insert(id.clone(), neigh);
    }

    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    let mut parent_map: HashMap<String, String> = HashMap::new();

    visited.insert(root_node_id.clone());
    queue.push_back((root_node_id.clone(), 0usize));

    while let Some((current_id, current_depth)) = queue.pop_front() {
        if let Some(max_d) = max_depth {
            if current_depth >= max_d { continue; }
        }
        if let Some(neighbors) = adjacency.get(&current_id) {
            for to_id in neighbors {
                if !visited.contains(to_id) {
                    visited.insert(to_id.clone());
                    parent_map.insert(to_id.clone(), current_id.clone());
                    if to_id == &target_node_id {
                        let mut path_ids = Vec::new();
                        let mut cur = target_node_id.clone();
                        path_ids.push(cur.clone());
                        while let Some(parent) = parent_map.get(&cur) {
                            path_ids.push(parent.clone());
                            cur = parent.clone();
                        }
                        let path_set: HashSet<String> = path_ids.iter().cloned().collect();
                        let mut path_nodes = HashMap::<String, Py<Node>>::new();
                        for pid in &path_ids {
                            if let Some(original_node) = vertex.nodes.get(pid) {
                                let orig_ref = original_node.bind(py);
                                let attr: HashMap<String, Py<PyAny>> = orig_ref.getattr("attr")?.extract().unwrap_or_default();
                                let original_edges: Vec<Py<Edge>> = orig_ref.getattr("edges")?.extract().unwrap_or_default();
                                let mut filtered_edges = Vec::new();
                                for edge in original_edges {
                                    let edge_ref = edge.bind(py);
                                    let to_node: Py<Node> = edge_ref.getattr("to_node")?.extract()?;
                                    let to_id = to_node.bind(py).getattr("id")?.extract::<String>()?;
                                    if path_set.contains(&to_id) {
                                        filtered_edges.push(edge.clone_ref(py));
                                    }
                                }
                                let new_node = Py::new(py, Node::new(pid.clone(), Some(attr), Some(filtered_edges)))?;
                                path_nodes.insert(pid.clone(), new_node);
                            }
                        }
                        let result_vertex = Vertex::from_nodes(py, path_nodes);
                        return Py::new(py, result_vertex);
                    }
                    queue.push_back((to_id.clone(), current_depth + 1));
                }
            }
        }
    }

    Err(pyo3::exceptions::PyValueError::new_err(
        format!("Target node '{}' not reachable from '{}' within max_depth {:?}",
                target_node_id, root_node_id, max_depth)
    ))
}

