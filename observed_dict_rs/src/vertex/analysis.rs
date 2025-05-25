// vertex/analysis.rs

use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict};
use super::Vertex;

pub fn get_metadata(vertex: &Vertex, py: Python<'_>) -> PyResult<Py<PyAny>> {
    let dict = PyDict::new(py);
    
    // Count nodes
    dict.set_item("node_count", vertex.nodes.len())?;
    
    // Count edges
    let mut edge_count = 0;
    for node_py in vertex.nodes.values() {
        let node_ref = node_py.bind(py);
        let edges: Vec<Py<crate::Edge>> = node_ref.getattr("edges")?.extract()?;
        edge_count += edges.len();
    }
    dict.set_item("edge_count", edge_count)?;
    
    // Calculate average degree
    if !vertex.nodes.is_empty() {
        let avg_degree = (edge_count as f64) / (vertex.nodes.len() as f64);
        dict.set_item("average_degree", avg_degree)?;
    } else {
        dict.set_item("average_degree", 0.0)?;
    }
    
    // List node IDs
    let node_ids: Vec<String> = vertex.nodes.keys().cloned().collect();
    dict.set_item("node_ids", node_ids)?;
    
    Ok(dict.into())
}

pub fn to_networkx(vertex: &Vertex, py: Python<'_>) -> PyResult<Py<PyAny>> {
    // Import networkx
    let networkx = py.import("networkx")
        .map_err(|_| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
            "NetworkX is not available. Please install it with: pip install networkx"
        ))?;
    
    // Create a new directed graph
    let digraph = networkx.call_method0("DiGraph")?;
    
    // Add all nodes first
    for (node_id, _) in &vertex.nodes {
        digraph.call_method1("add_node", (node_id,))?;
    }
    
    // Then add node attributes
    for (node_id, node_py) in &vertex.nodes {
        let node_ref = node_py.bind(py);
        
        // Get node attributes and add them to the NetworkX node
        if let Ok(attr) = node_ref.getattr("attr") {
            // Get the nodes dict from the graph
            let nodes_dict = digraph.getattr("nodes")?;
            let node_dict = nodes_dict.get_item(node_id)?;
            
            // Extract attributes from the Python dict and add them to NetworkX node
            if let Ok(attr_dict) = attr.downcast::<PyDict>() {
                for item in attr_dict.items() {
                    let (key, value) = item.extract::<(String, Py<PyAny>)>()?;
                    node_dict.set_item(key, value)?;
                }
            }
        }
    }
    
    // Add all edges with their attributes
    for (node_id, node_py) in &vertex.nodes {
        let node_ref = node_py.bind(py);
        
        // Get edges from this node
        if let Ok(edges) = node_ref.getattr("edges") {
            if let Ok(edges_vec) = edges.extract::<Vec<Py<PyAny>>>() {
                for edge_py in edges_vec {
                    let edge_ref = edge_py.bind(py);
                    
                    // Get target node
                    if let Ok(to_node) = edge_ref.getattr("to_node") {
                        if let Ok(to_id) = to_node.getattr("id").and_then(|id| id.extract::<String>()) {
                            // Add edge first
                            digraph.call_method1("add_edge", (node_id, &to_id))?;
                            
                            // Then add edge attributes
                            if let Ok(edge_attr) = edge_ref.getattr("attr") {
                                // Get the edges dict from the graph
                                let edges_dict = digraph.getattr("edges")?;
                                let edge_dict = edges_dict.get_item((node_id, &to_id))?;
                                
                                // Extract attributes from the Python dict and add them to NetworkX edge
                                if let Ok(attr_dict) = edge_attr.downcast::<PyDict>() {
                                    for item in attr_dict.items() {
                                        let (key, value) = item.extract::<(String, Py<PyAny>)>()?;
                                        edge_dict.set_item(key, value)?;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    Ok(digraph.into())
}
