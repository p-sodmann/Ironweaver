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
    let average_degree = if graph.is_empty() { 0.0 } else { graph.edge_count() as f64 / graph.node_count() as f64 };
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
    // Labels and types become the attributes "labels" / "type", as networkx
    // has no such fields.
    for (ix, node) in graph.nodes() {
        let attr = node.data.attr.to_dict(py)?;
        if !node.labels().is_empty() {
            attr.set_item("labels", graph.label_names(ix).expect("live"))?;
        }
        node_items.append((node.id(), attr))?;
        for &e in node.out_edges() {
            let edge = graph.edge(e).expect("listed edges are live");
            let to = graph.node(edge.target()).expect("edge targets are live");
            let attr = edge.data.attr.to_dict(py)?;
            if let Some(t) = graph.edge_type_name(e) {
                attr.set_item("type", t)?;
            }
            edge_items.append((node.id(), to.id(), attr))?;
        }
    }

    digraph.call_method1("add_nodes_from", (node_items,))?;
    digraph.call_method1("add_edges_from", (edge_items,))?;
    Ok(digraph.into())
}

/// `Vertex.memory_usage(deep=False)`: the graph structure's bytes (see
/// `Graph::memory_usage`); with `deep`, plus `sys.getsizeof` of every
/// attribute / meta dict and the values in them (containers recursively),
/// each object counted once however often it is shared.
pub fn memory_usage(vertex: &Bound<'_, Vertex>, deep: bool) -> PyResult<usize> {
    let py = vertex.py();
    let (structure, dicts) = {
        let v = vertex.try_borrow()?;
        let g = &v.graph;
        let mut dicts: Vec<Py<PyDict>> = Vec::new();
        if deep {
            let mut add = |a: &crate::data::PyAttrs| dicts.extend(a.py().map(|d| d.clone_ref(py)));
            for (_, n) in g.nodes() {
                add(&n.data.attr);
                add(&n.data.meta);
            }
            for (_, e) in g.edges() {
                add(&e.data.attr);
                add(&e.data.meta);
            }
        }
        (g.memory_usage(), dicts)
    };
    if !deep {
        return Ok(structure);
    }
    // The graph isn't borrowed now: __sizeof__ may run Python code
    let getsizeof = py.import("sys")?.getattr("getsizeof")?;
    let mut seen = std::collections::HashSet::new();
    let mut stack: Vec<Bound<'_, PyAny>> = dicts.into_iter().map(|d| d.into_bound(py).into_any()).collect();
    let mut total = structure;
    while let Some(obj) = stack.pop() {
        if !seen.insert(obj.as_ptr() as usize) {
            continue;
        }
        total += getsizeof.call1((&obj,))?.extract::<usize>()?;
        if let Ok(d) = obj.cast::<PyDict>() {
            for (k, x) in d.iter() {
                stack.push(k);
                stack.push(x);
            }
        } else if let Ok(l) = obj.cast::<PyList>() {
            stack.extend(l.iter());
        } else if let Ok(t) = obj.cast::<pyo3::types::PyTuple>() {
            stack.extend(t.iter());
        }
    }
    Ok(total)
}
