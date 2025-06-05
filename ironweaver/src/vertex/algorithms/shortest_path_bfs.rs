// vertex/algorithms/shortest_path_bfs.rs

use pyo3::prelude::*;
use std::collections::HashMap;
use crate::{Node, Edge};
use super::super::core::Vertex;

pub fn shortest_path_bfs(
    vertex: &Vertex,
    py: Python<'_>,
    root_node_id: String,
    target_node_id: String,
    max_depth: Option<usize>
) -> PyResult<Py<Vertex>> {
    use std::collections::{VecDeque, HashMap as StdHashMap};
    
    // Get the root node
    let root_node = vertex
        .nodes
        .get(&root_node_id)
        .ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err(format!(
                "Root node with id '{}' not found",
                root_node_id
            ))
        })?
        .clone_ref(py);
    
    // Check if target exists in the graph
    if !vertex.nodes.contains_key(&target_node_id) {
        return Err(pyo3::exceptions::PyValueError::new_err(
            format!("Target node with id '{}' not found", target_node_id)
        ));
    }
    
    // Check if root is the target
    if root_node_id == target_node_id {
        let mut path_nodes = HashMap::<String, Py<Node>>::new();
        
        // Create a new node with no edges (since it's just a single node path)
        let original_node_ref = root_node.borrow(py);
        let attr = original_node_ref
            .attr
            .iter()
            .map(|(k, v)| (k.clone(), v.clone_ref(py)))
            .collect::<HashMap<String, Py<PyAny>>>();
        let new_node = Py::new(py, Node::new(root_node_id.clone(), Some(attr), Some(Vec::new())))?;
        path_nodes.insert(root_node_id, new_node);
        
        let result_vertex = Vertex::from_nodes(py, path_nodes);
        return Py::new(py, result_vertex);
    }
    
    let mut visited = std::collections::HashSet::<String>::new();
    let mut queue = VecDeque::new();
    let mut parent_map = StdHashMap::<String, String>::new();
    
    // Initialize queue with root node
    visited.insert(root_node_id.clone());
    queue.push_back((root_node.clone_ref(py), 0));
    
    // Perform BFS from the root node
    while let Some((current_node, current_depth)) = queue.pop_front() {
        // Check depth limit
        if let Some(max_d) = max_depth {
            if current_depth >= max_d {
                continue;
            }
        }

        // Get edges from current node
        let current_ref = current_node.borrow(py);
        let current_id = current_ref.id.clone();

        for edge in &current_ref.edges {
            let edge_ref = edge.borrow(py);
            let to_node_actual: Py<Node> = edge_ref.to_node.clone_ref(py);
            let to_id = to_node_actual.borrow(py).id.clone();
            
            // If not visited, mark and enqueue
            if !visited.contains(&to_id) {
                visited.insert(to_id.clone());
                parent_map.insert(to_id.clone(), current_id.clone());
                queue.push_back((to_node_actual, current_depth + 1));
                
                // If this is our target, reconstruct the path
                if to_id == target_node_id {
                    // Reconstruct the path from target back to root
                    let mut path_ids = Vec::new();
                    let mut current = target_node_id.clone();
                    path_ids.push(current.clone());
                    
                    // Trace back through parents to build the path
                    while let Some(parent) = parent_map.get(&current) {
                        path_ids.push(parent.clone());
                        current = parent.clone();
                    }
                    
                    // Create new vertex with path nodes, filtering edges to only include path connections
                    let mut path_nodes = HashMap::<String, Py<Node>>::new();
                    let path_set: std::collections::HashSet<String> = path_ids.iter().cloned().collect();
                    
                    for path_id in &path_ids {
                        if let Some(original_node) = vertex.nodes.get(path_id) {
                            let original_node_ref = original_node.borrow(py);

                            let attr = original_node_ref
                                .attr
                                .iter()
                                .map(|(k, v)| (k.clone(), v.clone_ref(py)))
                                .collect::<HashMap<String, Py<PyAny>>>();
                            let original_edges: Vec<Py<Edge>> = original_node_ref
                                .edges
                                .iter()
                                .map(|e| e.clone_ref(py))
                                .collect();
                            let mut filtered_edges = Vec::new();

                            for edge in original_edges {
                                let edge_ref = edge.borrow(py);
                                let edge_to_node: Py<Node> = edge_ref.to_node.clone_ref(py);
                                let edge_to_id = edge_to_node.borrow(py).id.clone();
                                
                                // Only include edge if target is also in the path
                                if path_set.contains(&edge_to_id) {
                                    filtered_edges.push(edge.clone_ref(py));
                                }
                            }
                            
                            // Create new node with filtered edges
                            let new_node = Py::new(py, Node::new(path_id.clone(), Some(attr), Some(filtered_edges)))?;
                            path_nodes.insert(path_id.clone(), new_node);
                        }
                    }
                    
                    let result_vertex = Vertex::from_nodes(py, path_nodes);
                    return Py::new(py, result_vertex);
                }
            }
        }
    }
    
    // Target not found within max_depth
    Err(pyo3::exceptions::PyValueError::new_err(
        format!("Target node '{}' not reachable from '{}' within max_depth {:?}", 
                target_node_id, root_node_id, max_depth)
    ))
}
