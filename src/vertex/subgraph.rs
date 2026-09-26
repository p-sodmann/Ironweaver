// vertex/subgraph.rs
//
// Shared helpers for algorithms that produce a new Vertex from a subset of
// another one (filter, expand, shortest paths).

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::collections::{HashMap, HashSet};

use crate::gc_pause::GcPause;
use crate::{Edge, Node};
use super::Vertex;

/// Which edges a traversal follows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Out,
    In,
    Both,
}

impl Direction {
    pub fn parse(direction: Option<&str>) -> PyResult<Self> {
        match direction.unwrap_or("out") {
            "out" => Ok(Direction::Out),
            "in" => Ok(Direction::In),
            "both" => Ok(Direction::Both),
            other => Err(pyo3::exceptions::PyValueError::new_err(format!(
                "direction must be 'out', 'in' or 'both', got '{}'",
                other
            ))),
        }
    }

    /// The direction that walks the same edges backwards.
    pub fn reversed(self) -> Self {
        match self {
            Direction::Out => Direction::In,
            Direction::In => Direction::Out,
            Direction::Both => Direction::Both,
        }
    }
}

/// Neighbours of `node` reachable along one edge in `direction`, as
/// `(edge, neighbour)` pairs. Reads the Rust fields directly.
pub fn neighbors(py: Python<'_>, node: &Py<Node>, direction: Direction) -> Vec<(Py<Edge>, Py<Node>)> {
    let n = node.borrow(py);
    let mut out = Vec::new();
    if direction != Direction::In {
        for e in &n.edges {
            out.push((e.clone_ref(py), e.borrow(py).to_node.clone_ref(py)));
        }
    }
    if direction != Direction::Out {
        for e in &n.inverse_edges {
            out.push((e.clone_ref(py), e.borrow(py).from_node.clone_ref(py)));
        }
    }
    out
}

fn clone_map(py: Python<'_>, map: &HashMap<String, Py<PyAny>>) -> HashMap<String, Py<PyAny>> {
    map.iter().map(|(k, v)| (k.clone(), v.clone_ref(py))).collect()
}

/// Point every node and edge of `vertex` at it (back-reference) and at its
/// vertex-level update callback lists, exactly as `Vertex.add_node` /
/// `Vertex.add_edge` do for incrementally built graphs.
pub fn wire_vertex(py: Python<'_>, vertex: &Py<Vertex>) {
    let v = vertex.borrow(py);
    let v_any: Py<PyAny> = vertex.clone_ref(py).into_any();
    for node in v.nodes.values() {
        let mut n = node.borrow_mut(py);
        n.on_update_callbacks = v.on_node_update_callbacks.clone_ref(py);
        n.vertex = Some(v_any.clone_ref(py));
        for edge in &n.edges {
            let mut e = edge.borrow_mut(py);
            e.on_update_callbacks = v.on_edge_update_callbacks.clone_ref(py);
            e.vertex = Some(v_any.clone_ref(py));
        }
    }
}

/// Build a new Vertex containing copies of the nodes of `source` whose ids
/// are in `ids`, plus copies of every edge between two such nodes.
///
/// Each node is created exactly once. Every copied edge is registered in both
/// its source node's `edges` and its target node's `inverse_edges`. Node and
/// edge `attr`/`meta` dicts are shallow-copied (values are shared).
///
/// If `share_callbacks` is true the new vertex reuses the callback lists of
/// `source`; otherwise it gets fresh, empty lists. `meta` becomes the new
/// vertex's `meta` dict.
pub fn build_subgraph(
    py: Python<'_>,
    source: &Vertex,
    ids: &HashSet<String>,
    meta: Py<PyDict>,
    share_callbacks: bool,
) -> PyResult<Py<Vertex>> {
    let _gc = GcPause::new(py);
    let mut new_nodes: HashMap<String, Py<Node>> = HashMap::with_capacity(ids.len());
    for id in ids {
        if let Some(src_node) = source.nodes.get(id) {
            let s = src_node.borrow(py);
            let node = Node {
                id: id.clone(),
                attr: clone_map(py, &s.attr),
                edges: Vec::new(),
                inverse_edges: Vec::new(),
                meta: clone_map(py, &s.meta),
                on_edge_add_callbacks: Vec::new(),
                on_update_callbacks: PyList::empty(py).into(),
                vertex: None,
            };
            new_nodes.insert(id.clone(), Py::new(py, node)?);
        }
    }

    for (id, new_node) in &new_nodes {
        let src_node = &source.nodes[id];
        let src_edges: Vec<Py<Edge>> = src_node.borrow(py).edges.iter().map(|e| e.clone_ref(py)).collect();
        for src_edge in src_edges {
            let edge = {
                let e = src_edge.borrow(py);
                let to_id = e.to_node.borrow(py).id.clone();
                let target = match new_nodes.get(&to_id) {
                    Some(t) => t,
                    None => continue,
                };
                let edge = Py::new(
                    py,
                    Edge {
                        id: e.id.clone(),
                        from_node: new_node.clone_ref(py),
                        to_node: target.clone_ref(py),
                        attr: clone_map(py, &e.attr),
                        watched_by: Vec::new(),
                        meta: clone_map(py, &e.meta),
                        on_meta_change_callbacks: Vec::new(),
                        on_update_callbacks: PyList::empty(py).into(),
                        vertex: None,
                    },
                )?;
                target.borrow_mut(py).inverse_edges.push(edge.clone_ref(py));
                edge
            };
            new_node.borrow_mut(py).edges.push(edge);
        }
    }

    let vertex = if share_callbacks {
        Vertex {
            nodes: new_nodes,
            meta,
            on_node_add_callbacks: source.on_node_add_callbacks.clone_ref(py),
            on_edge_add_callbacks: source.on_edge_add_callbacks.clone_ref(py),
            on_node_update_callbacks: source.on_node_update_callbacks.clone_ref(py),
            on_edge_update_callbacks: source.on_edge_update_callbacks.clone_ref(py),
        }
    } else {
        let mut v = Vertex::from_nodes(py, new_nodes);
        v.meta = meta;
        v
    };
    let vertex = Py::new(py, vertex)?;
    wire_vertex(py, &vertex);
    Ok(vertex)
}
