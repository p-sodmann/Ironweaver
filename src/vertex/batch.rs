// vertex/batch.rs
//
// `Vertex.project`, and `Vertex.shortest_paths` / `Vertex.distances`: many
// queries in one call on a one-off projection. The projection is collected
// with the GIL held (reading weights needs it); sorting and the queries run
// on all cores with the GIL released. The Vertex stays borrowed meanwhile,
// so it can't change.

use ironweaver_core::batch;
use ironweaver_core::pathfinding::{check_max_cost, edge_cost, resolve, MethodKind};
use ironweaver_core::{Direction, NodeIx};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::collections::HashSet;

use super::Vertex;
use crate::errors::graph_error;
use crate::projection::{self, distances_to_py, paths_to_py, Filter, Projection, Spec};

/// `Vertex.project(...)`.
#[allow(clippy::too_many_arguments)]
pub fn project(
    slf: &Bound<'_, Vertex>,
    weight: Option<String>,
    default_weight: Option<f64>,
    direction: Option<&str>,
    nodes: Option<Vec<String>>,
    node_filter: Option<&Bound<'_, PyAny>>,
    edge_filter: Option<&Bound<'_, PyAny>>,
) -> PyResult<Projection> {
    let py = slf.py();
    let spec = Spec {
        direction: Direction::parse(direction).map_err(graph_error)?,
        cost: projection::edge_cost(weight, default_weight),
        nodes,
        node_filter: Filter::parse(py, node_filter, "node_filter")?,
        edge_filter: Filter::parse(py, edge_filter, "edge_filter")?,
    };
    let handle = slf.clone().unbind();
    let raw = {
        let vertex = slf.try_borrow()?;
        projection::collect(py, &vertex, Some(&handle), &spec)?
    };
    Ok(Projection { inner: py.detach(|| raw.finish()) })
}

/// Validate the shared options (same rules and messages as
/// `shortest_path`): the projection spec and whether the method uses weights.
fn options<'py>(
    py: Python<'py>,
    method: Option<&str>,
    weight: Option<String>,
    default_weight: Option<f64>,
    max_cost: Option<f64>,
    direction: Option<&str>,
) -> PyResult<(Spec<'py>, bool)> {
    let method = resolve(method, weight.is_some(), &[]).map_err(graph_error)?;
    if method.kind == MethodKind::AStar {
        return Err(PyValueError::new_err(
            "method 'astar' is not available for batch queries; use 'bfs' or 'dijkstra'",
        ));
    }
    let cost = edge_cost(method, weight, default_weight).map_err(graph_error)?;
    check_max_cost(max_cost).map_err(graph_error)?;
    let direction = Direction::parse(direction).map_err(graph_error)?;
    let no_filter = || Filter::parse(py, None, "");
    let spec = Spec { direction, cost, nodes: None, node_filter: no_filter()?, edge_filter: no_filter()? };
    Ok((spec, method.weighted))
}

/// Project the whole vertex (sorting with the GIL released).
fn project_all(vertex: &Vertex, py: Python<'_>, spec: &Spec<'_>) -> PyResult<ironweaver_core::Projection> {
    let raw = projection::collect(py, vertex, None, spec)?;
    Ok(py.detach(|| raw.finish()))
}

fn lookup(vertex: &Vertex, id: &str, role: &str) -> PyResult<NodeIx> {
    vertex.graph.node_ix(id).ok_or_else(|| PyValueError::new_err(format!("{} node with id '{}' not found", role, id)))
}

/// Dense index of a node of the (whole-graph) projection.
fn dense(p: &ironweaver_core::Projection, ix: NodeIx) -> u32 {
    p.index_of(ix).expect("every node of the vertex is projected")
}

#[allow(clippy::too_many_arguments)]
pub fn shortest_paths(
    vertex: &Vertex,
    py: Python<'_>,
    pairs: Vec<(String, String)>,
    method: Option<&str>,
    weight: Option<String>,
    default_weight: Option<f64>,
    max_cost: Option<f64>,
    direction: Option<&str>,
) -> PyResult<Py<PyList>> {
    let (spec, weighted) = options(py, method, weight, default_weight, max_cost, direction)?;
    let pairs: Vec<(NodeIx, NodeIx)> = pairs
        .iter()
        .map(|(s, t)| Ok((lookup(vertex, s, "Root")?, lookup(vertex, t, "Target")?)))
        .collect::<PyResult<_>>()?;
    let p = project_all(vertex, py, &spec)?;
    let pairs: Vec<(u32, u32)> = pairs.iter().map(|&(s, t)| (dense(&p, s), dense(&p, t))).collect();
    let results = py.detach(|| batch::shortest_paths(&p, &pairs, weighted, max_cost)).map_err(graph_error)?;
    paths_to_py(py, &p, results, weighted)
}

#[allow(clippy::too_many_arguments)]
pub fn distances(
    vertex: &Vertex,
    py: Python<'_>,
    sources: Vec<String>,
    targets: Option<Vec<String>>,
    method: Option<&str>,
    weight: Option<String>,
    default_weight: Option<f64>,
    max_cost: Option<f64>,
    direction: Option<&str>,
) -> PyResult<Py<PyDict>> {
    let (spec, weighted) = options(py, method, weight, default_weight, max_cost, direction)?;
    let sources: Vec<NodeIx> = sources.iter().map(|s| lookup(vertex, s, "Root")).collect::<PyResult<_>>()?;
    let targets: Option<Vec<NodeIx>> =
        targets.map(|ts| ts.iter().map(|t| lookup(vertex, t, "Target")).collect::<PyResult<_>>()).transpose()?;
    let p = project_all(vertex, py, &spec)?;
    let sources: Vec<u32> = sources.iter().map(|&s| dense(&p, s)).collect();
    let targets: Option<HashSet<u32>> = targets.map(|ts| ts.iter().map(|&t| dense(&p, t)).collect());
    let results = py.detach(|| batch::distances(&p, &sources, weighted, max_cost)).map_err(graph_error)?;
    distances_to_py(py, &p, &sources, results, targets.as_ref(), weighted)
}
