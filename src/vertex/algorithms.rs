// vertex/algorithms.rs
//
// Python front ends of the core algorithms that return new graphs or walks.

use ironweaver_core::random_walks::{plan, WalkOptions};
use ironweaver_core::traversal;
use ironweaver_core::{Direction, NodeIx};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::collections::HashSet;

use super::subgraph::build_subgraph;
use super::Vertex;
use crate::data::AttrMap;
use crate::errors::{graph_error, Error};
use crate::expr::PyExpr;
use crate::gc_pause::GcPause;
use crate::interrupt;

/// `Vertex.expand`: the source's nodes within `depth` edges of this
/// vertex's nodes (one multi-source BFS over `source`).
pub fn expand(
    vertex: &Vertex,
    py: Python<'_>,
    source: &Vertex,
    depth: Option<usize>,
    direction: Option<&str>,
) -> PyResult<Py<Vertex>> {
    let direction = Direction::parse(direction).map_err(graph_error)?;
    let seeds = vertex.graph.nodes().filter_map(|(_, n)| source.graph.node_ix(n.id()));
    let found = interrupt::polling(py, || traversal::expand(&source.graph, seeds, depth.unwrap_or(1), direction))?;
    build_subgraph(py, source, found, PyDict::new(py).unbind(), false)
}

/// `Vertex.filter(ids=..., id=..., where=Expr, **attr)`: the selected nodes
/// and the edges between them. The result shares `meta` and the callback
/// lists.
pub fn filter(vertex: &Vertex, py: Python<'_>, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Py<Vertex>> {
    let no_criteria = || PyValueError::new_err("Must specify ids, id, where, or attribute filters");
    let mut filters: AttrMap = kwargs.ok_or_else(no_criteria)?.extract()?;

    let keep = if let Some(expr) = filters.remove("where") {
        let expr = expr
            .bind(py)
            .cast::<PyExpr>()
            .map_err(|_| pyo3::exceptions::PyTypeError::new_err("where= takes an Expr (attr(...) > 1, label(...))"))?
            .get()
            .inner
            .clone();
        let candidates: Vec<NodeIx> = match vertex.graph.index_candidates(&expr)? {
            Some(c) => c,
            None => vertex.graph.node_indices().collect(),
        };
        let mut matches = Vec::new();
        for ix in candidates {
            if expr.matches_node(&vertex.graph, ix)? {
                matches.push(ix);
            }
        }
        matches
    } else if let Some(ids) = filters.remove("ids") {
        by_ids(vertex, ids.extract(py)?)?
    } else if let Some(id) = filters.remove("id") {
        by_ids(vertex, vec![id.extract(py)?])?
    } else if !filters.is_empty() {
        let mut matches = Vec::new();
        'nodes: for (ix, node) in vertex.graph.nodes() {
            for (key, expected) in &filters {
                match node.data.attr.get(py, key)? {
                    Some(value) if value.eq(expected.bind(py))? => {}
                    _ => continue 'nodes,
                }
            }
            matches.push(ix);
        }
        matches
    } else {
        return Err(no_criteria());
    };
    build_subgraph(py, vertex, keep, vertex.meta.clone_ref(py), true)
}

fn by_ids(vertex: &Vertex, ids: Vec<String>) -> PyResult<Vec<ironweaver_core::NodeIx>> {
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let ix = vertex
            .graph
            .node_ix(&id)
            .ok_or_else(|| PyValueError::new_err(format!("Node with id '{}' not found in vertex", id)))?;
        if seen.insert(ix) {
            out.push(ix);
        }
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
pub fn random_walks(
    vertex: &Vertex,
    py: Python<'_>,
    start_node_id: Option<String>,
    max_length: usize,
    min_length: Option<usize>,
    num_attempts: usize,
    allow_revisit: Option<bool>,
    include_edge_types: Option<bool>,
    edge_type_field: Option<String>,
    stratified: Option<bool>,
    seed: Option<u64>,
) -> PyResult<Py<PyList>> {
    let mut opts = WalkOptions::new(max_length, num_attempts);
    opts.min_length = min_length.unwrap_or(1);
    opts.allow_revisit = allow_revisit.unwrap_or(false);
    opts.include_edge_types = include_edge_types.unwrap_or(false);
    if let Some(field) = edge_type_field {
        opts.edge_type_field = field;
    }
    opts.stratified = stratified.unwrap_or(false);
    opts.seed = seed;

    let plan = plan::<_, _, Error>(&vertex.graph, start_node_id.as_deref(), opts)?;
    // The walks run on a detached index, so the GIL is released meanwhile
    // (always interruptible: the plan doesn't tell how long they take).
    let walks = interrupt::released(py, usize::MAX, || plan.run())?;

    let _gc = GcPause::new(py);
    let result = PyList::empty(py);
    for walk in &walks {
        let items: Vec<&str> = plan.items(walk).collect();
        result.append(PyList::new(py, items)?)?;
    }
    Ok(result.unbind())
}
