// vertex/batch.rs
//
// `Vertex.shortest_paths` / `Vertex.distances`: many queries in one call.
// The graph structure and edge costs are copied into a core `Snapshot`
// (reading weights needs the GIL), then the queries run on all cores with
// the GIL released. The Vertex stays borrowed meanwhile, so it can't change.

use ironweaver_core::batch::{self, Snapshot};
use ironweaver_core::pathfinding::{check_max_cost, edge_cost, resolve, MethodKind, PathMethod};
use ironweaver_core::{Direction, NodeIx};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::collections::HashSet;

use super::Vertex;
use crate::errors::{graph_error, Error};
use crate::gc_pause::GcPause;

/// Validate the shared options (same rules and messages as `shortest_path`).
fn setup(
    method: Option<&str>,
    weight: Option<String>,
    default_weight: Option<f64>,
    max_cost: Option<f64>,
    direction: Option<&str>,
) -> PyResult<(&'static PathMethod, ironweaver_core::pathfinding::EdgeCost, Direction)> {
    let method = resolve(method, weight.is_some(), &[]).map_err(graph_error)?;
    if method.kind == MethodKind::AStar {
        return Err(PyValueError::new_err(
            "method 'astar' is not available for batch queries; use 'bfs' or 'dijkstra'",
        ));
    }
    let cost = edge_cost(method, weight, default_weight).map_err(graph_error)?;
    check_max_cost(max_cost).map_err(graph_error)?;
    let direction = Direction::parse(direction).map_err(graph_error)?;
    Ok((method, cost, direction))
}

fn lookup(vertex: &Vertex, id: &str, role: &str) -> PyResult<NodeIx> {
    vertex.graph.node_ix(id).ok_or_else(|| {
        PyValueError::new_err(format!("{} node with id '{}' not found", role, id))
    })
}

fn id(vertex: &Vertex, ix: NodeIx) -> &str {
    vertex.graph.node(ix).expect("snapshot nodes are live while the vertex is borrowed").id()
}

/// A cost as Python sees it: an int for BFS (edge count), a float otherwise.
fn cost_object(py: Python<'_>, method: &PathMethod, cost: f64) -> PyResult<PyObject> {
    Ok(if method.weighted {
        cost.into_pyobject(py)?.into_any().unbind()
    } else {
        (cost as usize).into_pyobject(py)?.into_any().unbind()
    })
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
    let (method, cost, direction) = setup(method, weight, default_weight, max_cost, direction)?;
    let pairs: Vec<(NodeIx, NodeIx)> = pairs
        .iter()
        .map(|(s, t)| Ok((lookup(vertex, s, "Root")?, lookup(vertex, t, "Target")?)))
        .collect::<PyResult<_>>()?;
    let snapshot = Snapshot::build::<_, _, Error>(&vertex.graph, direction, &cost)?;
    let results = py
        .allow_threads(|| batch::shortest_paths(&snapshot, &pairs, max_cost))
        .map_err(graph_error)?;

    let _gc = GcPause::new(py);
    let out = PyList::empty(py);
    for result in results {
        match result {
            None => out.append(py.None())?,
            Some(path) => {
                let entry = PyDict::new(py);
                let ids: Vec<&str> = path.nodes.iter().map(|&n| id(vertex, n)).collect();
                entry.set_item("nodelist", ids)?;
                entry.set_item("cost", cost_object(py, method, path.cost)?)?;
                out.append(entry)?;
            }
        }
    }
    Ok(out.unbind())
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
    let (method, cost, direction) = setup(method, weight, default_weight, max_cost, direction)?;
    let sources: Vec<NodeIx> = sources.iter().map(|s| lookup(vertex, s, "Root")).collect::<PyResult<_>>()?;
    let targets: Option<HashSet<NodeIx>> = targets
        .map(|ts| ts.iter().map(|t| lookup(vertex, t, "Target")).collect::<PyResult<_>>())
        .transpose()?;
    let snapshot = Snapshot::build::<_, _, Error>(&vertex.graph, direction, &cost)?;
    let results = py
        .allow_threads(|| batch::distances(&snapshot, &sources, max_cost))
        .map_err(graph_error)?;

    let _gc = GcPause::new(py);
    let out = PyDict::new(py);
    for (&source, reached) in sources.iter().zip(results) {
        let per_source = PyDict::new(py);
        for (node, c) in reached {
            if targets.as_ref().is_none_or(|t| t.contains(&node)) {
                per_source.set_item(id(vertex, node), cost_object(py, method, c)?)?;
            }
        }
        out.set_item(id(vertex, source), per_source)?;
    }
    Ok(out.unbind())
}
