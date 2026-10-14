// vertex/batch.rs
//
// `Vertex.project`, and `Vertex.shortest_paths` / `Vertex.distances`: many
// queries in one call on a one-off projection. The projection is collected
// with the GIL held (reading weights needs it); sorting and the queries run
// on all cores with the GIL released. The Vertex stays borrowed meanwhile,
// so it can't change.

use ironweaver_core::batch;
use ironweaver_core::pathfinding::{check_max_cost, edge_cost, Coords};
use ironweaver_core::{Direction, NodeIx};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::collections::HashSet;

use super::pathfinding::parse_coords;
use super::Vertex;
use crate::errors::graph_error;
use crate::interrupt;
use crate::projection::results::distances_to_py;
use crate::projection::{
    self, resolve_batch_method, run_shortest_paths, AStarArgs, BatchMethod, Filter, Projection, Query, Spec,
};

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
    coords: Option<&Bound<'_, PyAny>>,
) -> PyResult<Projection> {
    let py = slf.py();
    let coords = coords.filter(|c| !c.is_none()).map(parse_coords).transpose()?;
    let spec = Spec {
        direction: Direction::parse(direction).map_err(graph_error)?,
        cost: projection::edge_cost(weight, default_weight),
        nodes,
        node_filter: Filter::parse(py, node_filter, "node_filter")?,
        edge_filter: Filter::parse(py, edge_filter, "edge_filter")?,
        coords,
    };
    let handle = slf.clone().unbind();
    let raw = {
        let vertex = slf.try_borrow()?;
        projection::collect(py, &vertex, Some(&handle), &spec)?
    };
    Ok(Projection { inner: py.detach(|| raw.finish()), vertex: slf.as_ptr() as usize })
}

/// Validate the shared options (same rules and messages as
/// `shortest_path`): the projection spec (with the coordinates A* reads,
/// `["x", "y"]` unless `coords` is given) and the method.
#[allow(clippy::too_many_arguments)]
fn options<'py>(
    method: Option<&str>,
    weight: Option<String>,
    default_weight: Option<f64>,
    max_cost: Option<f64>,
    direction: Option<&str>,
    query: Query,
    astar: AStarArgs<'_>,
    coords: Option<&Bound<'py, PyAny>>,
) -> PyResult<(Spec<'py>, BatchMethod)> {
    let method = resolve_batch_method(method, weight.is_some(), query, astar)?;
    let cost = edge_cost(method.method, weight, default_weight).map_err(graph_error)?;
    check_max_cost(max_cost).map_err(graph_error)?;
    let direction = Direction::parse(direction).map_err(graph_error)?;
    let coords = match (&method.astar, coords) {
        (None, _) => None,
        (Some(_), Some(c)) => Some(parse_coords(c)?),
        (Some(_), None) => Some(Coords::default()),
    };
    let spec = Spec { direction, cost, nodes: None, node_filter: Filter::All, edge_filter: Filter::All, coords };
    Ok((spec, method))
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
    coords: Option<&Bound<'_, PyAny>>,
    heuristic: Option<&str>,
    heuristic_scale: Option<f64>,
) -> PyResult<Py<PyList>> {
    let coords = coords.filter(|c| !c.is_none());
    let astar = AStarArgs { heuristic, scale: heuristic_scale, coords: coords.is_some() };
    let (spec, method) = options(method, weight, default_weight, max_cost, direction, Query::Pairs, astar, coords)?;
    let pairs: Vec<(NodeIx, NodeIx)> = pairs
        .iter()
        .map(|(s, t)| Ok((lookup(vertex, s, "Root")?, lookup(vertex, t, "Target")?)))
        .collect::<PyResult<_>>()?;
    let p = project_all(vertex, py, &spec)?;
    let pairs: Vec<(u32, u32)> = pairs.iter().map(|&(s, t)| (dense(&p, s), dense(&p, t))).collect();
    run_shortest_paths(py, &p, &pairs, &method, max_cost)
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
    let (spec, method) =
        options(method, weight, default_weight, max_cost, direction, Query::Distances, AStarArgs::default(), None)?;
    let weighted = method.method.weighted;
    let sources: Vec<NodeIx> = sources.iter().map(|s| lookup(vertex, s, "Root")).collect::<PyResult<_>>()?;
    let targets: Option<Vec<NodeIx>> =
        targets.map(|ts| ts.iter().map(|t| lookup(vertex, t, "Target")).collect::<PyResult<_>>()).transpose()?;
    let p = project_all(vertex, py, &spec)?;
    let sources: Vec<u32> = sources.iter().map(|&s| dense(&p, s)).collect();
    let targets: Option<HashSet<u32>> = targets.map(|ts| ts.iter().map(|&t| dense(&p, t)).collect());
    let results = interrupt::released(py, interrupt::size(&p), || batch::distances(&p, &sources, weighted, max_cost))?
        .map_err(graph_error)?;
    distances_to_py(py, &p, &sources, results, targets.as_ref(), weighted)
}
