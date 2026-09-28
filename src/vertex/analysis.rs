// vertex/analysis.rs

use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};

use super::Vertex;
use crate::gc_pause::GcPause;

pub fn get_metadata(vertex: &Vertex, py: Python<'_>) -> PyResult<Py<PyAny>> {
    let dict = PyDict::new(py);
    let graph = &vertex.graph;
    dict.set_item("node_count", graph.node_count())?;
    dict.set_item("edge_count", graph.edge_count())?;
    let average_degree = if graph.is_empty() {
        0.0
    } else {
        graph.edge_count() as f64 / graph.node_count() as f64
    };
    dict.set_item("average_degree", average_degree)?;
    let node_ids: Vec<&str> = graph.nodes().map(|(_, n)| n.id()).collect();
    dict.set_item("node_ids", node_ids)?;
    Ok(dict.into())
}

pub fn to_networkx(vertex: &Vertex, py: Python<'_>) -> PyResult<Py<PyAny>> {
    let networkx = py.import("networkx").map_err(|_| {
        PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
            "NetworkX is not available. Please install it with: pip install networkx",
        )
    })?;

    let _gc = GcPause::new(py);
    let digraph = networkx.call_method0("DiGraph")?;

    // Build (id, attr) and (from, to, attr) lists and hand them to networkx
    // in two bulk calls instead of several Python calls per node and edge.
    let graph = &vertex.graph;
    let node_items = PyList::empty(py);
    let edge_items = PyList::empty(py);
    for (_, node) in graph.nodes() {
        node_items.append((node.id(), node.data.attr.to_dict(py)?))?;
        for &e in node.out_edges() {
            let edge = graph.edge(e).expect("listed edges are live");
            let to = graph.node(edge.target()).expect("edge targets are live");
            edge_items.append((node.id(), to.id(), edge.data.attr.to_dict(py)?))?;
        }
    }

    digraph.call_method1("add_nodes_from", (node_items,))?;
    digraph.call_method1("add_edges_from", (edge_items,))?;
    Ok(digraph.into())
}
