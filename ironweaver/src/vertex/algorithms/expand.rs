// vertex/algorithms/expand.rs

use pyo3::prelude::*;
use std::collections::HashMap;
use crate::{Node, Edge};
use super::super::core::Vertex;

pub fn expand(
    vertex: &Vertex,
    py: Python<'_>,
    source_vertex: &Vertex,
    depth: Option<usize>
) -> PyResult<Py<Vertex>> {
    use std::collections::{VecDeque, HashSet};
    
    let expansion_depth = depth.unwrap_or(1);
    let mut discovered_node_ids = HashSet::<String>::new();
    
    // Start with all nodes from the current vertex
    for node_id in vertex.nodes.keys() {
        discovered_node_ids.insert(node_id.clone());
    }
    
    // For each node in the current vertex, perform BFS expansion from the source vertex
    for current_node_id in vertex.nodes.keys() {
        // Find the corresponding node in the source vertex
        if let Some(source_node) = source_vertex.nodes.get(current_node_id) {
            let mut visited = HashSet::<String>::new();
            let mut queue = VecDeque::new();
            
            // Start BFS from the current node
            visited.insert(current_node_id.clone());
            queue.push_back((source_node.clone_ref(py), 0));
            
            while let Some((current_node, current_depth)) = queue.pop_front() {
                // Stop if we've reached the expansion depth
                if current_depth >= expansion_depth {
                    continue;
                }

                // Get edges from current node
                let current_ref = current_node.borrow(py);
                for edge in &current_ref.edges {
                    let edge_ref = edge.borrow(py);
                    let to_node: Py<Node> = edge_ref.to_node.clone_ref(py);
                    let to_id = to_node.borrow(py).id.clone();
                    
                    // If we haven't visited this node in this BFS traversal
                    if !visited.contains(&to_id) {
                        visited.insert(to_id.clone());
                        
                        // Add to discovered nodes (this will include it in the final result)
                        discovered_node_ids.insert(to_id.clone());
                        
                        // Continue BFS from this node if we have more depth to explore
                        if current_depth + 1 < expansion_depth {
                            if let Some(source_neighbor) = source_vertex.nodes.get(&to_id) {
                                queue.push_back((source_neighbor.clone_ref(py), current_depth + 1));
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Now create the result vertex with all discovered nodes and their filtered edges
    let mut result_nodes = HashMap::<String, Py<Node>>::new();
    
    for node_id in &discovered_node_ids {
        // Get the node from the source vertex (which has the complete node data)
        if let Some(source_node) = source_vertex.nodes.get(node_id) {
            let source_node_ref = source_node.borrow(py);

            let attr = source_node_ref
                .attr
                .iter()
                .map(|(k, v)| (k.clone(), v.clone_ref(py)))
                .collect::<HashMap<String, Py<PyAny>>>();

            let source_edges: Vec<Py<Edge>> = source_node_ref
                .edges
                .iter()
                .map(|e| e.clone_ref(py))
                .collect();
            
            // Filter edges to only include those pointing to nodes that are also in our result set
            let mut filtered_edges = Vec::new();
            for edge in source_edges {
                let edge_ref = edge.borrow(py);
                let to_node: Py<Node> = edge_ref.to_node.clone_ref(py);
                let to_id = to_node.borrow(py).id.clone();
                
                // Only include edge if target is also in the discovered nodes
                if discovered_node_ids.contains(&to_id) {
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
        let node_ref = node.borrow(py);
        let attr = node_ref
            .attr
            .iter()
            .map(|(k, v)| (k.clone(), v.clone_ref(py)))
            .collect::<HashMap<String, Py<PyAny>>>();
        let edges: Vec<Py<Edge>> = node_ref
            .edges
            .iter()
            .map(|e| e.clone_ref(py))
            .collect();
        
        // Create new edges with proper node references from our result set
        let mut updated_edges = Vec::new();
        for edge in edges {
            let edge_ref = edge.borrow(py);
            let to_node: Py<Node> = edge_ref.to_node.clone_ref(py);
            let to_id = to_node.borrow(py).id.clone();
            
            // Get the target node from our result set
            if let Some(target_node) = result_nodes.get(&to_id) {
                let edge_attr: HashMap<String, Py<PyAny>> = edge_ref
                    .attr
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone_ref(py)))
                    .collect();
                let edge_id: Option<String> = edge_ref.id.clone();
                
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
