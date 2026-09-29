// vertex/pathfinding.rs
//
// `Vertex.shortest_path`: turns the Python arguments (including the
// method-specific `**options`) into a core `PathQuery`, runs it and builds
// the result Vertex. The algorithms and their registry live in
// `ironweaver_core::pathfinding` (see the recipe at the top of its mod.rs).

use ironweaver_core::pathfinding::{
    check_max_cost, check_options, edge_cost, find_path, not_reachable, resolve, Coords, Heuristic, MethodKind, Metric,
    PathQuery, METHODS,
};
use ironweaver_core::{Direction, NodeIx};
use pyo3::conversion::FromPyObjectOwned;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyString};

use super::subgraph::build_subgraph;
use super::Vertex;
use crate::data::{EdgeData, NodeData};
use crate::errors::{graph_error, Error};

/// Algorithm-specific keyword options (entries set to `None` are absent).
struct Options<'py>(Option<Bound<'py, PyDict>>);

impl<'py> Options<'py> {
    fn raw(&self, name: &str) -> PyResult<Option<Bound<'py, PyAny>>> {
        match &self.0 {
            Some(d) => Ok(d.get_item(name)?.filter(|v| !v.is_none())),
            None => Ok(None),
        }
    }

    fn get<T: FromPyObjectOwned<'py>>(&self, name: &str) -> PyResult<Option<T>> {
        match self.raw(name)? {
            Some(v) => v.extract::<T>().map(Some).map_err(|e| {
                let e: PyErr = e.into();
                PyTypeError::new_err(format!("invalid value for option '{}': {}", name, e))
            }),
            None => Ok(None),
        }
    }

    fn names(&self) -> PyResult<Vec<String>> {
        let mut out = Vec::new();
        if let Some(d) = &self.0 {
            for (k, v) in d.iter() {
                if !v.is_none() {
                    out.push(k.extract::<String>()?);
                }
            }
        }
        Ok(out)
    }
}

/// `Vertex.path_methods()`: {name: description}.
pub fn path_methods(py: Python<'_>) -> PyResult<Py<PyDict>> {
    let d = PyDict::new(py);
    for m in METHODS {
        d.set_item(m.name, m.description)?;
    }
    Ok(d.into())
}

type PyHeuristic<'h> = Heuristic<'h, NodeData, Error>;

/// A*'s heuristic from its `heuristic` / `coords` / `distances` options.
fn heuristic<'py>(
    py: Python<'py>,
    vertex: &Vertex,
    options: &Options<'py>,
    target: NodeIx,
    target_id: &str,
) -> PyResult<PyHeuristic<'py>> {
    let metric = options.get::<String>("heuristic")?;
    let coords = options.raw("coords")?;

    if let Some(name) = options.get::<String>("distances")? {
        if metric.is_some() || coords.is_some() {
            return Err(PyValueError::new_err("use either distances=... or heuristic/coords=..., not both"));
        }
        let table = vertex
            .meta
            .bind(py)
            .get_item(&name)?
            .ok_or_else(|| PyValueError::new_err(format!("distances: vertex.meta has no key '{}'", name)))?;
        let table = table.cast_into::<PyDict>().map_err(|_| {
            PyTypeError::new_err(format!(
                "distances: vertex.meta['{}'] must be a dict {{node_id: estimate}} \
                 or {{node_id: {{target_id: estimate}}}}",
                name
            ))
        })?;
        let target_id = target_id.to_owned();
        // Precomputed estimates: {node_id: estimate} or {node_id: {target_id: estimate}}
        return Ok(Heuristic::Custom(Box::new(move |node| {
            let entry = match table.get_item(node.id())? {
                Some(v) if !v.is_none() => v,
                _ => return Ok(0.0),
            };
            let value = match entry.cast::<PyDict>() {
                Ok(per_target) => match per_target.get_item(&target_id)? {
                    Some(v) if !v.is_none() => v,
                    _ => return Ok(0.0),
                },
                Err(_) => entry,
            };
            value.extract::<f64>().map_err(|_| {
                Error(PyTypeError::new_err(format!(
                    "distances: vertex.meta['{}'] entry for node '{}' must be a number \
                     or a dict {{target_id: number}}",
                    name,
                    node.id()
                )))
            })
        })));
    }

    let metric = Metric::parse(metric.as_deref()).map_err(graph_error)?;
    let coords = match coords {
        None => Coords::default(),
        Some(v) => match v.cast::<PyString>() {
            Ok(s) => Coords::sequence(&s.to_cow()?),
            Err(_) => {
                let paths: Vec<String> = v.extract().map_err(|_| {
                    PyTypeError::new_err(
                        "coords must be an attribute name holding a sequence (e.g. \"pos\") \
                         or a list of attribute paths, one per dimension (e.g. [\"x\", \"y\"] or [\"pos.lat\", \"pos.lon\"])",
                    )
                })?;
                Coords::per_dimension(&paths).map_err(graph_error)?
            }
        },
    };
    Ok(Heuristic::coords(&vertex.graph, target, metric, coords)?)
}

/// Entry point behind `Vertex.shortest_path` (and the `shortest_path_bfs` /
/// `shortest_path_dijkstra` shorthands).
#[allow(clippy::too_many_arguments)]
pub fn shortest_path<'py>(
    vertex: &Vertex,
    py: Python<'py>,
    source_id: String,
    target_id: String,
    method: Option<&str>,
    weight: Option<String>,
    default_weight: Option<f64>,
    max_cost: Option<f64>,
    direction: Option<&str>,
    options: Option<Bound<'py, PyDict>>,
) -> PyResult<Py<Vertex>> {
    let options = Options(options);
    let names = options.names()?;
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let method = resolve(method, weight.is_some(), &names).map_err(graph_error)?;
    check_options(method, &names).map_err(graph_error)?;
    let cost = edge_cost(method, weight, default_weight).map_err(graph_error)?;
    check_max_cost(max_cost).map_err(graph_error)?;
    let direction = Direction::parse(direction).map_err(graph_error)?;

    let graph = &vertex.graph;
    let source = graph
        .node_ix(&source_id)
        .ok_or_else(|| PyValueError::new_err(format!("Root node with id '{}' not found", source_id)))?;
    let target = graph
        .node_ix(&target_id)
        .ok_or_else(|| PyValueError::new_err(format!("Target node with id '{}' not found", target_id)))?;

    let max_depth = options.get::<usize>("max_depth")?;
    let heuristic = match method.kind {
        MethodKind::AStar => heuristic(py, vertex, &options, target, &target_id)?,
        _ => Heuristic::Zero,
    };
    let mut query = PathQuery { method, direction, cost, max_cost, max_depth, heuristic };
    let result = find_path::<NodeData, EdgeData, Error>(graph, source, target, &mut query)?;
    let result = match result {
        Some(r) => r,
        None => return Err(graph_error(not_reachable(method, &source_id, &target_id, max_depth, max_cost))),
    };

    // The path's nodes and every edge between them, with meta: nodelist
    // (source -> target order), cost, method and, when known, expanded.
    let meta = PyDict::new(py);
    let ids: Vec<&str> = result.nodes.iter().map(|&n| graph.node(n).expect("path nodes are live").id()).collect();
    meta.set_item("nodelist", ids)?;
    if method.weighted {
        meta.set_item("cost", result.cost)?;
    } else {
        meta.set_item("cost", result.nodes.len() - 1)?;
    }
    meta.set_item("method", method.name)?;
    if let Some(n) = result.expanded {
        meta.set_item("expanded", n)?;
    }
    build_subgraph(py, vertex, result.nodes, meta.unbind(), false)
}
