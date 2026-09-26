// vertex/algorithms/shortest_path_bfs.rs

use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::collections::{HashMap, HashSet, VecDeque};
use super::super::core::Vertex;
use super::super::subgraph::{build_subgraph, neighbors, Direction};
use super::bidirectional::{bidirectional_bfs, is_library_built};

/// Build the result vertex for a path: the path nodes, every edge between
/// them, and `meta["nodelist"]` holding the path in root -> target order.
pub(crate) fn path_vertex(
    py: Python<'_>,
    vertex: &Vertex,
    path_ids: Vec<String>,
) -> PyResult<Py<Vertex>> {
    let path_set: HashSet<String> = path_ids.iter().cloned().collect();
    let meta = PyDict::new(py);
    meta.set_item("nodelist", path_ids)?;
    build_subgraph(py, vertex, &path_set, meta.into(), false)
}

pub fn shortest_path_bfs(
    vertex: &Vertex,
    py: Python<'_>,
    root_node_id: String,
    target_node_id: String,
    max_depth: Option<usize>,
    direction: Option<&str>,
) -> PyResult<Py<Vertex>> {
    let direction = Direction::parse(direction)?;

    // Get the root node
    let root_node = vertex.nodes.get(&root_node_id)
        .ok_or_else(|| pyo3::exceptions::PyValueError::new_err(
            format!("Root node with id '{}' not found", root_node_id)
        ))?
        .clone_ref(py);

    // Check if target exists in the graph
    if !vertex.nodes.contains_key(&target_node_id) {
        return Err(pyo3::exceptions::PyValueError::new_err(
            format!("Target node with id '{}' not found", target_node_id)
        ));
    }

    // Check if root is the target
    if root_node_id == target_node_id {
        return path_vertex(py, vertex, vec![root_node_id]);
    }

    // Graphs built through the library keep edges/inverse_edges in sync, so
    // they can be searched from both ends at once.
    if is_library_built(py, &root_node) {
        let target_node = vertex.nodes[&target_node_id].clone_ref(py);
        let path = bidirectional_bfs(py, &root_node, &target_node, max_depth, direction, |_, _| Ok(true))?;
        return match path {
            Some(nodes) => {
                let path_ids = nodes.iter().map(|n| n.borrow(py).id.clone()).collect();
                path_vertex(py, vertex, path_ids)
            }
            None => Err(not_reachable(&root_node_id, &target_node_id, max_depth)),
        };
    }

    // Hand-built graphs: one-sided BFS over outgoing/incoming lists only
    let mut visited = HashSet::<String>::new();
    let mut queue = VecDeque::new();
    let mut parent_map = HashMap::<String, String>::new();

    // Initialize queue with root node
    visited.insert(root_node_id.clone());
    queue.push_back((root_node, 0));

    // Perform BFS from the root node
    while let Some((current_node, current_depth)) = queue.pop_front() {
        // Check depth limit
        if let Some(max_d) = max_depth {
            if current_depth >= max_d {
                continue;
            }
        }

        let current_id = current_node.borrow(py).id.clone();

        for (_edge, neighbor) in neighbors(py, &current_node, direction) {
            let to_id = neighbor.borrow(py).id.clone();

            // If not visited, mark and enqueue
            if visited.insert(to_id.clone()) {
                parent_map.insert(to_id.clone(), current_id.clone());

                // If this is our target, reconstruct the path
                if to_id == target_node_id {
                    let mut path_ids = vec![to_id];
                    while let Some(parent) = parent_map.get(path_ids.last().unwrap()) {
                        path_ids.push(parent.clone());
                    }
                    path_ids.reverse(); // built target→root; reverse to root→target
                    return path_vertex(py, vertex, path_ids);
                }

                queue.push_back((neighbor, current_depth + 1));
            }
        }
    }

    // Target not found within max_depth
    Err(not_reachable(&root_node_id, &target_node_id, max_depth))
}

fn not_reachable(root: &str, target: &str, max_depth: Option<usize>) -> PyErr {
    pyo3::exceptions::PyValueError::new_err(format!(
        "Target node '{}' not reachable from '{}' within max_depth {:?}",
        target, root, max_depth
    ))
}
