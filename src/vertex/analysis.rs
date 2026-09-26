// vertex/analysis.rs

use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};
use super::Vertex;
use crate::gc_pause::GcPause;

pub fn get_metadata(vertex: &Vertex, py: Python<'_>) -> PyResult<Py<PyAny>> {
    let dict = PyDict::new(py);
    
    // Count nodes
    dict.set_item("node_count", vertex.nodes.len())?;
    
    // Count edges
    let edge_count: usize = vertex.nodes.values().map(|n| n.borrow(py).edges.len()).sum();
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
    
    let _gc = GcPause::new(py);

    // Create a new directed graph
    let digraph = networkx.call_method0("DiGraph")?;

    // Build (id, attr) and (from, to, attr) lists and hand them to networkx
    // in two bulk calls instead of several Python calls per node and edge.
    let node_items = PyList::empty(py);
    let edge_items = PyList::empty(py);
    for (node_id, node_py) in &vertex.nodes {
        let node = node_py.borrow(py);
        let attr = PyDict::new(py);
        for (k, v) in &node.attr {
            attr.set_item(k, v)?;
        }
        node_items.append((node_id, attr))?;

        for edge_py in &node.edges {
            let edge = edge_py.borrow(py);
            let to_id = edge.to_node.borrow(py).id.clone();
            let attr = PyDict::new(py);
            for (k, v) in &edge.attr {
                attr.set_item(k, v)?;
            }
            edge_items.append((node_id, to_id, attr))?;
        }
    }

    digraph.call_method1("add_nodes_from", (node_items,))?;
    digraph.call_method1("add_edges_from", (edge_items,))?;

    Ok(digraph.into())
}
