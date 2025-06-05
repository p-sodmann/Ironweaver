// vertex/algorithms/parallel_bfs.rs

use pyo3::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use rayon::prelude::*;

use crate::{Node, Edge};
use super::super::core::Vertex;

/// Perform a parallel breadth-first search starting from `root_node_id`.
///
/// This implementation explores each BFS frontier in parallel using
/// `rayon` to distribute work across threads. Python objects are accessed
/// under the GIL on each thread.
pub fn parallel_bfs(
    vertex: &Vertex,
    py: Python<'_>,
    root_node_id: String,
    max_depth: Option<usize>
) -> PyResult<Py<Vertex>> {
    use std::collections::HashSet;

    // Validate root existence
    if !vertex.nodes.contains_key(&root_node_id) {
        return Err(pyo3::exceptions::PyValueError::new_err(
            format!("Root node with id '{}' not found", root_node_id)
        ));
    }

    // Shared structures across threads
    let visited = Arc::new(Mutex::new(HashSet::<String>::new()));
    let found = Arc::new(Mutex::new(HashMap::<String, Py<Node>>::new()));
    let nodelist = Arc::new(Mutex::new(Vec::<String>::new()));

    // Seed with the root node
    Python::with_gil(|gil| {
        if let Some(node) = vertex.nodes.get(&root_node_id) {
            found.lock().unwrap().insert(root_node_id.clone(), node.clone_ref(gil));
        }
    });
    visited.lock().unwrap().insert(root_node_id.clone());
    nodelist.lock().unwrap().push(root_node_id.clone());

    let mut frontier = vec![root_node_id];
    let mut depth = 0usize;

    while !frontier.is_empty() {
        if let Some(max_d) = max_depth {
            if depth >= max_d { break; }
        }

        let next_frontier = Arc::new(Mutex::new(Vec::<String>::new()));
        frontier.par_iter().for_each(|node_id| {
            Python::with_gil(|gil| {
                if let Some(node) = vertex.nodes.get(node_id) {
                    let node_ref = node.bind(gil);
                    if let Ok(edges) = node_ref.getattr("edges").and_then(|e| e.extract::<Vec<Py<Edge>>>()) {
                        for edge in edges {
                            if let Ok(to_node) = edge.bind(gil).getattr("to_node").and_then(|o| o.extract::<Py<Node>>()) {
                                if let Ok(to_id) = to_node.bind(gil).getattr("id").and_then(|o| o.extract::<String>()) {
                                    let mut visited_lock = visited.lock().unwrap();
                                    if !visited_lock.contains(&to_id) {
                                        visited_lock.insert(to_id.clone());
                                        drop(visited_lock);

                                        found.lock().unwrap().insert(to_id.clone(), to_node.clone_ref(gil));
                                        nodelist.lock().unwrap().push(to_id.clone());
                                        next_frontier.lock().unwrap().push(to_id);
                                    }
                                }
                            }
                        }
                    }
                }
            });
        });

        frontier = next_frontier.lock().unwrap().clone();
        depth += 1;
    }

    let result_nodes = Arc::try_unwrap(found).unwrap().into_inner().unwrap();
    let path = Arc::try_unwrap(nodelist).unwrap().into_inner().unwrap();
    let result_vertex = Vertex::from_nodes_with_path(py, result_nodes, path)?;
    Py::new(py, result_vertex)
}

